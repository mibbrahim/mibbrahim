"use client";

import { useCallback, useState, type ReactNode } from "react";
import { CameraScanner, type ScanSide } from "@/components/CameraScanner";
import { Icon, type IconName } from "@/components/Icon";
import { Select } from "@/components/Select";
import { Stepper, TopNav } from "@/components/TopNav";
import { isAamva, parseAamva } from "@/lib/aamva";
import { compressImage, readCardText, readLicenseBarcode } from "@/lib/image";
import { parseInsuranceText } from "@/lib/insurance";
import { OTHER_PLAN, type Check, type IntakeForm } from "@/lib/types";

type Step = "license" | "insurance" | "confirm" | "done" | "byPhone";
const FLOW: Step[] = ["license", "insurance", "confirm"];
const FLOW_LABELS = ["License", "Insurance", "Confirm"];

type Result = { code: string; status: "verified" | "needs_review" | "cash_pay"; checks: Check[] };
type Notice = { tone: "verified" | "pending" | "failed"; text: string } | null;
type ImageKey = keyof IntakeForm["images"];

const LICENSE_SIDES: ScanSide[] = [
  { key: "licenseFront", label: "Front", hint: "Fit the front of your license inside the frame, then tap the button" },
  { key: "licenseBack", label: "Back", hint: "Now flip it over — the barcode scans by itself" },
];
const INSURANCE_SIDES: ScanSide[] = [
  { key: "insuranceFront", label: "Front", hint: "Fit the front of your card inside the frame, then tap the button" },
  { key: "insuranceBack", label: "Back", hint: "Now the back of your card (optional)" },
];

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

export function Kiosk({ plans, initialPhone }: { plans: string[]; initialPhone: string }) {
  // The phone number comes from the texted link; only ask for it if it's missing.
  const [askPhone] = useState(() => initialPhone.replace(/\D/g, "").length < 10);
  const [step, setStep] = useState<Step>("license");
  const [form, setForm] = useState<IntakeForm>(() => emptyForm(initialPhone));
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<ImageKey | "submit" | "">("");
  const [licenseNote, setLicenseNote] = useState<Notice>(null);
  const [cardNote, setCardNote] = useState<Notice>(null);
  const [ocrPct, setOcrPct] = useState<number | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  // Live camera unless it can't be opened, then fall back to the phone's photo picker.
  const [camError, setCamError] = useState<string | null>(null);
  const [retake, setRetake] = useState<ImageKey | null>(null);

  const setLic = (patch: Partial<IntakeForm["license"]>) =>
    setForm((f) => ({ ...f, license: { ...f.license, ...patch } }));
  const setIns = (patch: Partial<IntakeForm["insurance"]>) =>
    setForm((f) => ({ ...f, insurance: { ...f.insurance, ...patch } }));

  const go = (s: Step) => {
    setError("");
    setStep(s);
    window.scrollTo({ top: 0 });
  };

  // Barcode check run on live camera frames while the back of the license is in view.
  const detectBarcode = useCallback(async (frame: ImageData) => {
    const text = await readLicenseBarcode(frame, true);
    return text && isAamva(text) ? text : null;
  }, []);

  function scanSide(sides: ScanSide[]): ScanSide | null {
    return sides.find((s) => s.key === retake) ?? sides.find((s) => !form.images[s.key as ImageKey]) ?? null;
  }

  function onCapture(key: string, file: File, detected?: string) {
    setRetake(null);
    onPhoto(key as ImageKey, file, detected);
  }

  async function onPhoto(key: ImageKey, file: File | undefined, detected?: string) {
    if (!file) return;
    setError("");
    setBusy(key);
    let image: string;
    try {
      image = await compressImage(file);
      setForm((f) => ({ ...f, images: { ...f.images, [key]: image } }));
    } catch {
      setBusy("");
      return setError("Couldn't load that photo. Please try again.");
    }
    setBusy("");

    if (key === "licenseBack") {
      setLicenseNote({ tone: "pending", text: "Reading the barcode on your license…" });
      const text = detected ?? (await readLicenseBarcode(file).catch(() => null));
      if (text && isAamva(text)) {
        const { licenseNumber, ...rest } = parseAamva(text);
        const patch = Object.fromEntries(
          Object.entries({ ...rest, number: licenseNumber }).filter(([, v]) => v),
        ) as Partial<IntakeForm["license"]>;
        setLic({ ...patch, scanned: true });
        setLicenseNote({ tone: "verified", text: `Got it${patch.firstName ? `, ${patch.firstName}` : ""}! We read your details from the barcode.` });
      } else {
        setLicenseNote({ tone: "failed", text: "We couldn't read the barcode. Retake the back in good light, or continue and type your details." });
      }
    }

    if (key === "insuranceFront" || key === "insuranceBack") {
      setCardNote({ tone: "pending", text: "Reading your insurance card…" });
      setOcrPct(0);
      try {
        // OCR a sharper copy than the upload; small print needs the detail.
        const text = await readCardText(await compressImage(file, 2000, 0.92), setOcrPct);
        const g = parseInsuranceText(text, plans);
        setForm((f) => {
          const ins = { ...f.insurance };
          if (g.memberId && !ins.memberId) ins.memberId = g.memberId;
          if (g.groupNumber && !ins.groupNumber) ins.groupNumber = g.groupNumber;
          if (!ins.plan && g.plan) ins.plan = g.plan;
          else if (!ins.plan && g.payer) { ins.plan = OTHER_PLAN; ins.otherPlanName = g.payer; }
          return { ...f, insurance: ins };
        });
        const found = [g.payer, g.memberId && `ID ${g.memberId}`].filter(Boolean).join(" · ");
        setCardNote(found
          ? { tone: "verified", text: `Found ${found}. You can check it on the next screen.` }
          : { tone: "pending", text: "Card saved. We couldn't read the text clearly — you can type it on the next screen." });
      } catch {
        setCardNote({ tone: "pending", text: "Card saved. You can type your member ID on the next screen." });
      } finally {
        setOcrPct(null);
      }
    }
  }

  function next(from: Step) {
    const f = form;
    if (from === "license") {
      if (!f.images.licenseFront) return setError("Take a photo of the front of your license.");
      if (!f.images.licenseBack) return setError("Take a photo of the back of your license — the barcode fills in your details.");
      return go("insurance");
    }
    if (from === "insurance") {
      if (!f.cashPay && !f.images.insuranceFront) return setError("Take a photo of the front of your insurance card, or choose cash pay.");
      return go("confirm");
    }
  }

  async function submit() {
    const l = form.license;
    if (askPhone && form.phone.replace(/\D/g, "").length !== 10) return setError("Enter your 10-digit mobile number.");
    if (!l.firstName || !l.lastName || !l.dob || !l.number || !l.expiration || !l.street || !l.city || !l.zip)
      return setError("Please fill in all your details.");
    if (!form.cashPay) {
      if (!form.insurance.plan) return setError("Choose your insurance plan.");
      if (!form.insurance.memberId) return setError("Enter your insurance member ID.");
    }
    if (!consent) return setError("Please tick the box to confirm your details are correct.");
    setBusy("submit");
    setError("");
    try {
      const res = await fetch("/api/submit", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...form, dob: form.license.dob }),
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
  const back: Partial<Record<Step, Step>> = { insurance: "license", confirm: "insurance", byPhone: "license" };
  const planNotListed = form.insurance.plan === OTHER_PLAN;

  // Live viewfinder on top, captured front/back below. Falls back to the
  // photo picker when the camera can't be opened.
  function renderScanner(kind: "license" | "insurance", sides: ScanSide[]) {
    const active = scanSide(sides);
    if (camError) {
      return (
        <>
          <div className="notice notice--soft">
            <span className="icon"><Icon name="camera" size={18} color="#647689" /></span>
            <div className="notice__text">{camError} Tap below to take or choose a photo instead.</div>
          </div>
          <CardArt kind={kind} photo={form.images[sides[0].key as ImageKey]} />
          <div className="capture-grid">
            {sides.map((s) => (
              <Capture key={s.key} label={s.label} hint={s.key === "licenseBack" ? "Barcode side" : s.key === "insuranceBack" ? "Optional" : "Photo side"}
                value={form.images[s.key as ImageKey]} busy={busy === s.key} onFile={(f) => onPhoto(s.key as ImageKey, f)} />
            ))}
          </div>
        </>
      );
    }
    return (
      <>
        <CameraScanner kind={kind} side={active} done={!active}
          detect={kind === "license" && active?.key === "licenseBack" ? detectBarcode : undefined}
          onCapture={onCapture} onUnavailable={setCamError} />
        <div className="capture-grid app-shots">
          {sides.map((s) => {
            const img = form.images[s.key as ImageKey];
            const isActive = active?.key === s.key;
            return (
              <button key={s.key} type="button"
                className={`capture-tile app-shot ${img ? "is-done" : ""} ${isActive ? "is-active" : ""}`}
                onClick={() => setRetake(s.key as ImageKey)} aria-label={img ? `Retake ${s.label.toLowerCase()}` : `Scan ${s.label.toLowerCase()}`}>
                {/* eslint-disable-next-line @next/next/no-img-element */}
                {img && <img className="app-shot__img" src={img} alt="" />}
                <span className="capture-tile__corner">{s.label}</span>
                {busy === s.key ? <span className="spinner" /> : !img && (
                  <span className="capture-tile__icon"><Icon name="camera" size={18} /></span>
                )}
                <span className="capture-tile__text">
                  {busy === s.key ? "Saving…" : img ? (isActive ? "Retaking…" : "Saved · tap to retake") : isActive ? "Scanning now…" : "Waiting"}
                </span>
              </button>
            );
          })}
        </div>
      </>
    );
  }

  return (
    <>
      <TopNav tag="Patient Form" center={stepIndex >= 0 ? <Stepper steps={FLOW_LABELS} current={stepIndex} /> : undefined} />

      <main className="app-main">
        <div className="app-stack">
          {step === "license" && (
            <>
              <Header kicker="Step 1 of 3 · Fill out your details" title="Scan your driver's license"
                sub="Hold your license up to the camera — front first, then the back. We'll fill in your details for you." />
              {renderScanner("license", LICENSE_SIDES)}
              {licenseNote && <Banner tone={licenseNote.tone}>{licenseNote.text}</Banner>}
              <Tips items={["Put the card on a dark surface", "Good light, no glare", "Fill the frame with the card"]} />
              <button className="app-link" onClick={() => go("byPhone")}>I&apos;d rather do this over the phone</button>
            </>
          )}

          {step === "insurance" && (
            <>
              <Header kicker="Step 2 of 3" title="Scan your insurance card"
                sub="Hold your card up to the camera — front first, then the back. We'll read your plan and member ID." />
              {form.cashPay ? (
                <>
                  <div className="notice notice--gold">
                    <span className="icon"><Icon name="info" size={18} /></span>
                    <div className="notice__text">You&apos;re booking as <b>self-pay</b>. The agent will go over pricing with you.</div>
                  </div>
                  <button className="app-link" onClick={() => setForm({ ...form, cashPay: false })}>I have insurance — scan my card</button>
                </>
              ) : (
                <>
                  {renderScanner("insurance", INSURANCE_SIDES)}
                  {ocrPct !== null && (
                    <div className="app-ocr">
                      <div className="progress"><div className="progress__fill" style={{ width: `${Math.max(6, ocrPct)}%` }} /></div>
                      <span className="t-label">Reading your card… {ocrPct}%</span>
                    </div>
                  )}
                  {cardNote && ocrPct === null && <Banner tone={cardNote.tone}>{cardNote.text}</Banner>}
                  <button className="app-link" onClick={() => setForm({ ...form, cashPay: true })}>I don&apos;t have insurance — I&apos;ll pay cash</button>
                </>
              )}
            </>
          )}

          {step === "confirm" && (
            <>
              <Header kicker="Step 3 of 3" title="Are these details correct?"
                sub="We filled these in from your license and insurance card. Fix anything that's wrong." />

              <Section icon="user" title="Your details" auto={form.license.scanned}>
                {askPhone && (
                  <TextField label="Mobile number" type="tel" inputMode="tel" autoComplete="tel" value={form.phone}
                    onChange={(v) => setForm({ ...form, phone: formatPhone(v) })} />
                )}
                <div className="app-grid2">
                  <TextField label="First name" value={form.license.firstName} onChange={(v) => setLic({ firstName: v })} autoComplete="given-name" />
                  <TextField label="Last name" value={form.license.lastName} onChange={(v) => setLic({ lastName: v })} autoComplete="family-name" />
                </div>
                <TextField label="Date of birth" type="date" value={form.license.dob} onChange={(v) => setLic({ dob: v })} />
              </Section>

              <Section icon="building" title="Home address" auto={form.license.scanned}>
                <TextField label="Street address" value={form.license.street} onChange={(v) => setLic({ street: v })} autoComplete="address-line1" />
                <div className="app-grid3">
                  <TextField label="City" value={form.license.city} onChange={(v) => setLic({ city: v })} autoComplete="address-level2" />
                  <TextField label="State" value={form.license.state} onChange={(v) => setLic({ state: v.toUpperCase().slice(0, 2) })} />
                  <TextField label="ZIP" value={form.license.zip} inputMode="numeric" autoComplete="postal-code"
                    onChange={(v) => setLic({ zip: v.replace(/[^\d-]/g, "").slice(0, 10) })} />
                </div>
              </Section>

              <Section icon="card" title="Driver's license" auto={form.license.scanned}>
                <div className="app-grid2">
                  <TextField label="License number" value={form.license.number} placeholder="A1234567"
                    onChange={(v) => setLic({ number: v.toUpperCase().replace(/\s/g, "") })} />
                  <TextField label="Expires" type="date" value={form.license.expiration} onChange={(v) => setLic({ expiration: v })} />
                </div>
              </Section>

              <Section icon="shield" title="Insurance" auto={!form.cashPay && Boolean(form.insurance.memberId && cardNote?.tone === "verified")}>
                {form.cashPay ? (
                  <div className="app-row">
                    <span className="pill pill--gold">Self-pay</span>
                    <button className="app-link" onClick={() => go("insurance")}>Use insurance instead</button>
                  </div>
                ) : (
                  <>
                    <Field label="Insurance plan">
                      <Select value={form.insurance.plan} placeholder="Choose your plan…" onChange={(v) => setIns({ plan: v })}
                        options={[...plans.map((p) => ({ value: p, label: p })), { value: OTHER_PLAN, label: "My plan isn't listed" }]} />
                    </Field>
                    {planNotListed && (
                      <>
                        <TextField label="Your plan name" value={form.insurance.otherPlanName} onChange={(v) => setIns({ otherPlanName: v })} />
                        <div className="notice notice--gold">
                          <span className="icon"><Icon name="info" size={18} /></span>
                          <div className="notice__text">
                            We may not be in network with this plan. You can book as <b>self-pay</b>, or ask the agent on the phone.
                            <div style={{ marginTop: 10 }}>
                              <button className="btn btn--secondary btn--sm" onClick={() => setForm({ ...form, cashPay: true })}>Book as cash pay</button>
                            </div>
                          </div>
                        </div>
                      </>
                    )}
                    <div className="app-grid2">
                      <TextField label="Member ID" value={form.insurance.memberId} onChange={(v) => setIns({ memberId: v.toUpperCase() })} />
                      <TextField label="Group number" hint="Optional" value={form.insurance.groupNumber} onChange={(v) => setIns({ groupNumber: v.toUpperCase() })} />
                    </div>
                  </>
                )}
              </Section>

              <label className="opt-row app-consent">
                <input className="check check--lg" type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
                <span>These details are correct, and I agree to share them and my card photos with the office to verify my identity and insurance.</span>
              </label>
              <Hipaa />
            </>
          )}

          {step === "byPhone" && (
            <>
              <Header title="No problem" sub="Let the agent on the phone know. They'll stay on the line and go through your details with you." />
              <div className="notice notice--soft">
                <span className="icon"><Icon name="info" size={18} color="#647689" /></span>
                <div className="notice__text">You can also text a photo of your license and insurance card to the number that sent you this link.</div>
              </div>
            </>
          )}

          {step === "done" && result && (
            <>
              <div className="ds-h is-center app-done">
                {result.status === "needs_review" ? (
                  <span className="icon-tile app-done__icon"><Icon name="phone" size={30} /></span>
                ) : (
                  <span className="success-badge"><Icon name="check" size={34} sw={3} /></span>
                )}
                <h1 className="step-header__title">{result.status === "needs_review" ? "Thanks — almost there" : "You're all set"}</h1>
                <p className="step-header__sub">Read this code to the agent on the phone.</p>
              </div>
              <div className="card card--kiosk">
                <div className="t-caps" style={{ textAlign: "center", marginBottom: 6 }}>Your code</div>
                <div className="app-code">{result.code}</div>
              </div>
              {result.status === "needs_review" && (
                <div className="list app-checks">
                  {result.checks.filter((c) => !c.ok).map((c) => (
                    <div key={c.id} className="list-row">
                      <span className="app-check-icon is-bad"><Icon name="info" size={15} sw={2.4} /></span>
                      <div className="list-row__main">
                        <div className="list-row__title">{c.label}</div>
                        {c.detail && <div className="list-row__meta">{c.detail}</div>}
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div className="notice notice--soft">
                <span className="icon"><Icon name="info" size={18} color="#647689" /></span>
                <div className="notice__text">
                  {result.status === "needs_review" ? "The agent will go over these with you. You can close this page." : "You can close this page now."}
                </div>
              </div>
            </>
          )}

          {error && <Banner tone="failed">{error}</Banner>}
        </div>
      </main>

      {step !== "done" && (
        <div className="action-bar action-bar--blur app-bar">
          <div className="app-bar__inner">
            {back[step] && (
              <button className="btn btn--secondary btn--lg btn--icon" aria-label="Back" onClick={() => go(back[step]!)}>
                <Icon name="back" size={18} sw={2.2} />
              </button>
            )}
            {step === "confirm" ? (
              <button className="btn btn--cta app-cta" onClick={submit} disabled={busy === "submit"}>
                {busy === "submit" ? "Sending…" : "Yes, submit"} <Icon name="check" size={16} sw={2.6} color="#fff" />
              </button>
            ) : step !== "byPhone" ? (
              <button className="btn btn--cta app-cta" onClick={() => next(step)} disabled={busy !== "" || ocrPct !== null}>
                {busy || ocrPct !== null ? "Please wait…" : "Next"} <Icon name="arrow" size={16} sw={2.4} color="#fff" />
              </button>
            ) : null}
          </div>
        </div>
      )}
    </>
  );
}

function Header({ kicker, title, sub }: { kicker?: string; title: string; sub?: string }) {
  return (
    <div>
      {kicker && <div className="step-header__kicker">{kicker}</div>}
      <h1 className="step-header__title">{title}</h1>
      {sub && <p className="step-header__sub">{sub}</p>}
    </div>
  );
}

// Illustration of the card to scan; shows the patient's photo once taken.
function CardArt({ kind, photo }: { kind: "license" | "insurance"; photo?: string }) {
  return (
    <div className={`app-cardart app-cardart--${kind} ${photo ? "has-photo" : ""}`} aria-hidden>
      {photo ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={photo} alt="" />
      ) : (
        <>
          <div className="app-cardart__top">{kind === "license" ? "CALIFORNIA · DRIVER LICENSE" : "HEALTH PLAN · MEMBER CARD"}</div>
          {kind === "license" && <div className="app-cardart__face"><Icon name="user" size={34} sw={1.4} color="rgba(255,255,255,.85)" /></div>}
          <div className="app-cardart__lines">
            <span style={{ width: "62%" }} /><span style={{ width: "44%" }} /><span style={{ width: "54%" }} />
          </div>
          <div className="app-cardart__scan" />
        </>
      )}
    </div>
  );
}

function Tips({ items }: { items: string[] }) {
  return (
    <ul className="app-tips">
      {items.map((t) => (
        <li key={t}><Icon name="check" size={14} sw={2.4} color="var(--primary)" /> {t}</li>
      ))}
    </ul>
  );
}

function Section({ icon, title, auto, children }: { icon: IconName; title: string; auto?: boolean; children: ReactNode }) {
  return (
    <section className="card card--kiosk card--flush">
      <div className="card__head">
        <span className="icon-tile icon-tile--sm"><Icon name={icon} size={16} /></span>
        <div className="card__title" style={{ flex: 1, fontSize: 15 }}>{title}</div>
        {auto && <span className="pill pill--sm pill--green"><Icon name="check" size={11} sw={2.6} /> Auto-filled</span>}
      </div>
      <div className="card__body app-stack app-stack--tight">{children}</div>
    </section>
  );
}

function Hipaa() {
  return (
    <div className="hipaa-bar">
      <span className="icon"><Icon name="lock" size={16} /></span>
      <div className="hipaa-bar__text">Your information is encrypted and handled under HIPAA. It&apos;s only used to verify your identity and insurance.</div>
    </div>
  );
}

function Banner({ tone, children }: { tone: "verified" | "pending" | "failed"; children: ReactNode }) {
  const icon: IconName = tone === "verified" ? "check" : tone === "failed" ? "close" : "info";
  const badge = tone === "verified" ? undefined : tone === "failed" ? { background: "#F04438", color: "#fff" } : { background: "var(--line)", color: "var(--ink-3)" };
  return (
    <div className={`verify-banner verify-banner--${tone}`} role={tone === "failed" ? "alert" : "status"}>
      <span className="verify-banner__badge" style={badge}><Icon name={icon} size={13} sw={3} /></span>
      <span>{children}</span>
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="field">
      <label className="field__label">{label}</label>
      {children}
      {hint && <span className="field__hint">{hint}</span>}
    </div>
  );
}

function TextField({ label, value, onChange, hint, ...rest }: {
  label: string; value: string; onChange: (v: string) => void; hint?: string;
} & Omit<React.InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  return (
    <Field label={label} hint={hint}>
      <div className="input input--kiosk">
        <input value={value} onChange={(e) => onChange(e.target.value)} {...rest} />
      </div>
    </Field>
  );
}

// Design-system capture tile: dashed → loading → captured (solid green).
function Capture({ label, hint, value, busy, onFile }: {
  label: string; hint: string; value?: string; busy: boolean; onFile: (f: File | undefined) => void;
}) {
  return (
    <label className={`capture-tile ${value ? "is-done" : ""}`}>
      <span className="capture-tile__corner">{label}</span>
      {busy ? (
        <span className="spinner" />
      ) : (
        <span className="capture-tile__icon"><Icon name={value ? "check" : "camera"} size={18} sw={value ? 2.4 : 1.6} /></span>
      )}
      <span className="capture-tile__text">{busy ? "Loading…" : value ? "Captured · retake" : `Scan ${label.toLowerCase()}`}</span>
      {!value && !busy && <span className="app-capture-hint">{hint}</span>}
      <input type="file" accept="image/*" capture="environment"
        onChange={(e) => { onFile(e.target.files?.[0]); e.target.value = ""; }} />
    </label>
  );
}
