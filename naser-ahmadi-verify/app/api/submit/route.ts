import { randomInt } from "node:crypto";
import { NextResponse } from "next/server";
import { acceptedPlans } from "@/lib/config";
import { saveSubmission } from "@/lib/store";
import { OTHER_PLAN, type IntakeForm, type Submission } from "@/lib/types";
import { normalizePhone, runChecks, statusFor } from "@/lib/validate";

const MAX_IMAGE_CHARS = 1_500_000; // ~1.1 MB JPEG after base64
const IMAGE_KEYS = ["licenseFront", "licenseBack", "insuranceFront", "insuranceBack"] as const;

function str(v: unknown, max = 200): string {
  return typeof v === "string" ? v.slice(0, max).trim() : "";
}

// Short code the patient reads to the agent on the phone. No 0/O/1/I.
function newCode(): string {
  const alphabet = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
  let s = "";
  for (let i = 0; i < 6; i++) s += alphabet[randomInt(alphabet.length)];
  return s;
}

export async function POST(req: Request) {
  let body: Partial<IntakeForm>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "Invalid request" }, { status: 400 });
  }

  const lic = (body.license ?? {}) as Partial<IntakeForm["license"]>;
  const ins = (body.insurance ?? {}) as Partial<IntakeForm["insurance"]>;
  const imgs = (body.images ?? {}) as Record<string, unknown>;
  const accepted = acceptedPlans();
  const plan = str(ins.plan);

  const images: Record<string, string> = {};
  for (const k of IMAGE_KEYS) {
    const v = imgs[k];
    if (typeof v === "string" && v.startsWith("data:image/") && v.length <= MAX_IMAGE_CHARS) images[k] = v;
  }

  const form: IntakeForm = {
    phone: normalizePhone(str(body.phone)) ?? str(body.phone, 20),
    dob: str(body.dob, 10),
    license: {
      firstName: str(lic.firstName),
      lastName: str(lic.lastName),
      number: str(lic.number, 20).toUpperCase(),
      dob: str(lic.dob, 10),
      expiration: str(lic.expiration, 10),
      street: str(lic.street),
      city: str(lic.city),
      state: str(lic.state, 2).toUpperCase(),
      zip: str(lic.zip, 10),
      scanned: lic.scanned === true,
    },
    insurance: {
      plan: plan === OTHER_PLAN || accepted.includes(plan) ? plan : OTHER_PLAN,
      otherPlanName: str(ins.otherPlanName),
      memberId: str(ins.memberId, 40),
      groupNumber: str(ins.groupNumber, 40),
    },
    cashPay: body.cashPay === true,
    images,
  };

  const checks = runChecks(form, accepted);
  const { images: _drop, ...formNoImages } = form;
  const sub: Submission = {
    code: newCode(),
    createdAt: new Date().toISOString(),
    form: formNoImages,
    imageKeys: Object.keys(images),
    checks,
    status: statusFor(checks, form.cashPay),
  };

  try {
    await saveSubmission(sub, images);
  } catch (e) {
    console.error(e);
    return NextResponse.json({ error: "Could not save. Please tell the agent on the phone." }, { status: 500 });
  }

  return NextResponse.json({ code: sub.code, status: sub.status, checks: sub.checks });
}
