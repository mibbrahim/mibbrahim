"use client";

import { useState } from "react";
import { isAamva, parseAamva } from "@/lib/aamva";
import { compressImage, readLicenseBarcode } from "@/lib/image";
import { OTHER_PLAN, type Check, type IntakeForm } from "@/lib/types";

type Step = "welcome" | "phone" | "identity" | "license" | "insurance" | "review" | "done" | "byPhone";
const FLOW: Step[] = ["phone", "identity", "license", "insurance", "review"];

type Result = { code: string; status: "verified" | "needs_review" | "cash_pay"; checks: Check[] };

function formatPhone(s: string): string {
  let d = s.replace(/\D/g, "");
  if (d.length === 11 && d.startsWith("1")) d = d.slice(1);
  d = d.slice(0, 10);
  if (d.length < 4) return d;
  if (d.length < 7) return `(${d.slice(0, 3)}) ${d.slice(3)}`;
  return `(${d.slice(0, 3)}) ${d.slice(3, 6)}-${d.slice(6)}`;
}

const emptyForm = (phone: string): IntakeForm => ({
  phone: formatPhone(phone),
  dob: "",
  license: {
    firstName: "", lastName: "", number: "", dob: "", expiration: "",
    street: "", city: "", state: "CA", zip: "", scanned: false,
  },
  insurance: { plan: "", otherPlanName: "", memberId: "", groupNumber: "" },
  cashPay: false,
  images: {},
});

export function Kiosk({ practice, plans, initialPhone }: { practice: string; plans: string[]; initialPhone: string }) {
  const [step, setStep] = useState<Step>("welcome");
  const [form, setForm] = useState<IntakeForm>(() => emptyForm(initialPhone));
  const [error, setError] = useState("");
  const [busy, setBusy] = useState("");
  const [scanNote, setScanNote] = useState("");
  const [result, setResult] = useState<Result | null>(null);

  const setLic = (patch: Partial<IntakeForm["license"]>) =>
    setForm((f) => ({ ...f, license: { ...f.license, ...patch } }));
  const setIns = (patch: Partial<IntakeForm["insurance"]>) =>
    setForm((f) => ({ ...f, insurance: { ...f.insurance, ...patch } }));
  const setImg = (key: keyof IntakeForm["images"], v: string | undefined) =>
    setForm((f) => ({ ...f, images: { ...f.images, [key]: v } }));

  const go = (s: Step) => {
    setError("");
    setStep(s);
    window.scrollTo({ top: 0 });
  };

  async function onPhoto(key: keyof IntakeForm["images"], file: File | undefined) {
    if (!file) return;
    setError("");
    setBusy(key);
    try {
      setImg(key, await compressImage(file));
      if (key === "licenseBack") {
        setScanNote("Reading barcode…");
        const text = await readLicenseBarcode(file).catch(() => null);
        if (text && isAamva(text)) {
          const d = parseAamva(text);
          const { licenseNumber, ...rest } = d;
          const patch = Object.fromEntries(
            Object.entries({ ...rest, number: licenseNumber }).filter(([, v]) => v),
          ) as Partial<IntakeForm["license"]>;
          setLic({ ...patch, scanned: true });
          setScanNote("Barcode read. Please check the details below.");
        } else {
          setScanNote("Couldn't read the barcode. Please type your details below, or retake the photo in good light.");
        }
      }
    } catch {
      setError("Couldn't load that photo. Please try again.");
    } finally {
      setBusy("");
    }
  }

  function next(from: Step) {
    const f = form;
    if (from === "phone") {
      if (f.phone.replace(/\D/g, "").length !== 10) return setError("Enter your 10-digit mobile number.");
      return go("identity");
    }
    if (from === "identity") {
      if (!f.dob) return setError("Enter your date of birth.");
      return go("license");
    }
    if (from === "license") {
      const l = f.license;
      if (!f.images.licenseFront) return setError("Take a photo of the front of your license.");
      if (!l.firstName || !l.lastName || !l.number || !l.dob || !l.expiration || !l.street || !l.city || !l.zip)
        return setError("Fill in all license details.");
      return go("insurance");
    }
    if (from === "insurance") {
      if (f.cashPay) return go("review");
      if (!f.insurance.plan) return setError("Choose your insurance plan, or pick cash pay.");
      if (f.insurance.plan === OTHER_PLAN) return setError("That plan isn't on our list. Choose cash pay, or tell the agent on the phone.");
      if (!f.insurance.memberId) return setError("Enter your member ID.");
      if (!f.images.insuranceFront) return setError("Take a photo of the front of your insurance card.");
      return go("review");
    }
  }

  async function submit() {
    setBusy("submit");
    setError("");
    try {
      const res = await fetch("/api/submit", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(form),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || "Something went wrong.");
      setResult(data);
      go("done");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Something went wrong. Please tell the agent on the phone.");
    } finally {
      setBusy("");
    }
  }

  const stepIndex = FLOW.indexOf(step);
  const planNotListed = form.insurance.plan === OTHER_PLAN;

  return (
    <main className="shell">
      <header className="brand">
        <div className="logo" aria-hidden>+</div>
        <div>
          <div className="practice">{practice}</div>
          <div className="sub">Patient verification</div>
        </div>
      </header>

      {stepIndex >= 0 && (
        <div className="progress" aria-label={`Step ${stepIndex + 1} of ${FLOW.length}`}>
          {FLOW.map((s, i) => (
            <span key={s} className={i <= stepIndex ? "on" : ""} />
          ))}
        </div>
      )}

      <section className="card">
        {step === "welcome" && (
          <>
            <h1>Let&apos;s verify your details</h1>
            <p className="lead">
              Takes about 2 minutes. You&apos;ll take a photo of your <b>California driver&apos;s license</b> and your{" "}
              <b>insurance card</b>.
            </p>
            <p className="note">Please stay on the line with our agent while you do this.</p>
            <button className="primary" onClick={() => go("phone")}>Start</button>
            <button className="link" onClick={() => go("byPhone")}>I&apos;d rather do this over the phone</button>
          </>
        )}

        {step === "byPhone" && (
          <>
            <h1>No problem</h1>
            <p className="lead">
              Let the agent on the phone know. They&apos;ll stay on the line and go through your details with you.
            </p>
            <p className="note">
              You can also text a photo of your license and insurance card to the number that sent you this link.
            </p>
            <button className="secondary" onClick={() => go("welcome")}>Back</button>
          </>
        )}

        {step === "phone" && (
          <>
            <h1>Confirm your phone number</h1>
            <label>
              Mobile number
              <input
                type="tel" inputMode="tel" autoComplete="tel" placeholder="(555) 555-5555"
                value={form.phone}
                onChange={(e) => setForm({ ...form, phone: formatPhone(e.target.value) })}
              />
            </label>
            <Nav onNext={() => next("phone")} onBack={() => go("welcome")} />
          </>
        )}

        {step === "identity" && (
          <>
            <h1>Verify your date of birth</h1>
            <label>
              Date of birth
              <input
                type="date" autoComplete="bday" max={new Date().toISOString().slice(0, 10)}
                value={form.dob}
                onChange={(e) => setForm({ ...form, dob: e.target.value })}
              />
            </label>
            <p className="note">We&apos;ll match this against your driver&apos;s license.</p>
            <Nav onNext={() => next("identity")} onBack={() => go("phone")} />
          </>
        )}

        {step === "license" && (
          <>
            <h1>Driver&apos;s license</h1>
            <p className="note">California driver&apos;s license or state ID. Lay it flat in good light.</p>
            <div className="photos">
              <Photo label="Front" value={form.images.licenseFront} busy={busy === "licenseFront"}
                onFile={(f) => onPhoto("licenseFront", f)} />
              <Photo label="Back (barcode)" value={form.images.licenseBack} busy={busy === "licenseBack"}
                onFile={(f) => onPhoto("licenseBack", f)} />
            </div>
            {scanNote && <p className="scan">{scanNote}</p>}
            <div className="grid2">
              <Field label="First name" value={form.license.firstName} onChange={(v) => setLic({ firstName: v })} autoComplete="given-name" />
              <Field label="Last name" value={form.license.lastName} onChange={(v) => setLic({ lastName: v })} autoComplete="family-name" />
            </div>
            <Field label="License number" value={form.license.number} placeholder="A1234567"
              onChange={(v) => setLic({ number: v.toUpperCase().replace(/\s/g, "") })} />
            <div className="grid2">
              <Field label="Date of birth on license" type="date" value={form.license.dob} onChange={(v) => setLic({ dob: v })} />
              <Field label="Expires" type="date" value={form.license.expiration} onChange={(v) => setLic({ expiration: v })} />
            </div>
            <Field label="Street address" value={form.license.street} onChange={(v) => setLic({ street: v })} autoComplete="address-line1" />
            <div className="grid3">
              <Field label="City" value={form.license.city} onChange={(v) => setLic({ city: v })} autoComplete="address-level2" />
              <Field label="State" value={form.license.state} onChange={(v) => setLic({ state: v.toUpperCase().slice(0, 2) })} />
              <Field label="ZIP" value={form.license.zip} inputMode="numeric"
                onChange={(v) => setLic({ zip: v.replace(/[^\d-]/g, "").slice(0, 10) })} autoComplete="postal-code" />
            </div>
            <Nav onNext={() => next("license")} onBack={() => go("identity")} />
          </>
        )}

        {step === "insurance" && (
          <>
            <h1>Insurance card</h1>
            {!form.cashPay && (
              <>
                <label>
                  Insurance plan
                  <select value={form.insurance.plan} onChange={(e) => setIns({ plan: e.target.value })}>
                    <option value="">Choose your plan…</option>
                    {plans.map((p) => (
                      <option key={p} value={p}>{p}</option>
                    ))}
                    <option value={OTHER_PLAN}>My plan isn&apos;t listed</option>
                  </select>
                </label>
                {planNotListed && (
                  <div className="warn">
                    <p>We may not be in network with your plan.</p>
                    <Field label="Your plan name" value={form.insurance.otherPlanName} onChange={(v) => setIns({ otherPlanName: v })} />
                    <p>Would you like to book as <b>cash pay</b> instead? Or ask the agent on the phone.</p>
                    <button className="secondary" onClick={() => setForm({ ...form, cashPay: true })}>Book as cash pay</button>
                  </div>
                )}
                {!planNotListed && (
                  <>
                    <div className="photos">
                      <Photo label="Front" value={form.images.insuranceFront} busy={busy === "insuranceFront"}
                        onFile={(f) => onPhoto("insuranceFront", f)} />
                      <Photo label="Back" value={form.images.insuranceBack} busy={busy === "insuranceBack"}
                        onFile={(f) => onPhoto("insuranceBack", f)} />
                    </div>
                    <div className="grid2">
                      <Field label="Member ID" value={form.insurance.memberId} onChange={(v) => setIns({ memberId: v })} />
                      <Field label="Group # (optional)" value={form.insurance.groupNumber} onChange={(v) => setIns({ groupNumber: v })} />
                    </div>
                  </>
                )}
                <button className="link" onClick={() => setForm({ ...form, cashPay: true })}>I don&apos;t have insurance / I&apos;ll pay cash</button>
              </>
            )}
            {form.cashPay && (
              <div className="warn">
                <p><b>Cash pay selected.</b> The agent will go over pricing with you.</p>
                <button className="link" onClick={() => setForm({ ...form, cashPay: false })}>Use insurance instead</button>
              </div>
            )}
            <Nav onNext={() => next("insurance")} onBack={() => go("license")} />
          </>
        )}

        {step === "review" && (
          <>
            <h1>Review</h1>
            <dl className="review">
              <dt>Phone</dt><dd>{form.phone}</dd>
              <dt>Name</dt><dd>{form.license.firstName} {form.license.lastName}</dd>
              <dt>Date of birth</dt><dd>{form.dob}</dd>
              <dt>License</dt><dd>{form.license.number} ({form.license.state}), expires {form.license.expiration}</dd>
              <dt>Address</dt><dd>{form.license.street}, {form.license.city}, {form.license.state} {form.license.zip}</dd>
              <dt>Payment</dt>
              <dd>{form.cashPay ? "Cash pay" : `${form.insurance.plan} · ID ${form.insurance.memberId}`}</dd>
            </dl>
            <p className="note">
              By submitting, you agree to share these details and photos with {practice} to verify your identity and
              insurance.
            </p>
            <Nav onNext={submit} nextLabel={busy === "submit" ? "Sending…" : "Submit"} disabled={busy === "submit"}
              onBack={() => go("insurance")} />
          </>
        )}

        {step === "done" && result && (
          <>
            <h1>{result.status === "needs_review" ? "Thanks — almost there" : "You're all set"}</h1>
            <p className="lead">Read this code to the agent on the phone:</p>
            <div className="code">{result.code}</div>
            <ul className="checks">
              {result.checks.map((c) => (
                <li key={c.id} className={c.ok ? "ok" : "bad"}>
                  <span aria-hidden>{c.ok ? "✓" : "!"}</span>
                  <div>
                    {c.label}
                    {!c.ok && c.detail && <small>{c.detail}</small>}
                  </div>
                </li>
              ))}
            </ul>
            {result.status === "needs_review" && (
              <p className="note">The agent will go over anything marked with “!” with you.</p>
            )}
          </>
        )}

        {error && <p className="error" role="alert">{error}</p>}
      </section>

      <footer className="foot">Your information is sent securely and is only used by {practice}.</footer>
    </main>
  );
}

function Nav({ onNext, onBack, nextLabel = "Next", disabled }: {
  onNext: () => void; onBack?: () => void; nextLabel?: string; disabled?: boolean;
}) {
  return (
    <div className="nav">
      {onBack && <button className="secondary" onClick={onBack}>Back</button>}
      <button className="primary" onClick={onNext} disabled={disabled}>{nextLabel}</button>
    </div>
  );
}

function Field({ label, value, onChange, ...rest }: {
  label: string; value: string; onChange: (v: string) => void;
} & Omit<React.InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  return (
    <label>
      {label}
      <input value={value} onChange={(e) => onChange(e.target.value)} {...rest} />
    </label>
  );
}

function Photo({ label, value, busy, onFile }: {
  label: string; value?: string; busy: boolean; onFile: (f: File | undefined) => void;
}) {
  return (
    <label className={`photo ${value ? "has" : ""}`}>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {value ? <img src={value} alt={label} /> : <span className="cam" aria-hidden>📷</span>}
      <span>{busy ? "Loading…" : value ? `${label} · retake` : `${label}`}</span>
      <input type="file" accept="image/*" capture="environment" hidden
        onChange={(e) => { onFile(e.target.files?.[0]); e.target.value = ""; }} />
    </label>
  );
}
