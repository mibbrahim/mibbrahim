"use client";

import { useCallback, useEffect, useState } from "react";
import { Icon } from "@/components/Icon";
import { TopNav } from "@/components/TopNav";
import type { Submission } from "@/lib/types";


const STATUS: Record<Submission["status"], { label: string; style: React.CSSProperties }> = {
  verified: { label: "Verified", style: { background: "var(--success-soft)", color: "var(--success)" } },
  needs_review: { label: "Needs review", style: { background: "var(--amber-soft)", color: "#8a5a07" } },
  cash_pay: { label: "Cash pay", style: { background: "var(--gold-soft)", color: "var(--gold-ink)" } },
};

const IMAGE_LABELS: Record<string, string> = {
  licenseFront: "License front",
  licenseBack: "License back",
  insuranceFront: "Insurance front",
  insuranceBack: "Insurance back",
};

function fmtPhone(d: string) {
  return d.length === 10 ? `(${d.slice(0, 3)}) ${d.slice(3, 6)}-${d.slice(6)}` : d;
}

function StatusChip({ status }: { status: Submission["status"] }) {
  const s = STATUS[status];
  return (
    <span className="grid-cell-status" style={s.style}>
      <Icon name={status === "verified" ? "check" : status === "cash_pay" ? "card" : "info"} size={12} sw={2.4} /> {s.label}
    </span>
  );
}

export default function AgentPage() {
  const [pin, setPin] = useState("");
  const [authed, setAuthed] = useState(false);
  const [list, setList] = useState<Submission[]>([]);
  const [durable, setDurable] = useState(true);
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState<{ submission: Submission; images: Record<string, string> } | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    try {
      const saved = sessionStorage.getItem("agentPin");
      if (saved) setPin(saved);
    } catch {}
  }, []);

  const api = useCallback(
    async (qs: string) => {
      const res = await fetch(`/api/submissions${qs}`, { headers: { "x-agent-pin": pin }, cache: "no-store" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || `Error ${res.status}`);
      return data;
    },
    [pin],
  );

  const refresh = useCallback(async () => {
    setError("");
    try {
      const digits = search.replace(/\D/g, "");
      const data = await api(digits.length >= 10 ? `?phone=${digits}` : "");
      setList(data.submissions);
      setDurable(data.durable);
      setAuthed(true);
      try { sessionStorage.setItem("agentPin", pin); } catch {}
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error");
    }
  }, [api, pin, search]);

  async function openCode(code: string) {
    setError("");
    try {
      setOpen(await api(`?code=${encodeURIComponent(code.trim().toUpperCase())}`));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error");
    }
  }

  function signOut() {
    try { sessionStorage.removeItem("agentPin"); } catch {}
    setPin("");
    setAuthed(false);
    setOpen(null);
    setList([]);
  }

  // Poll while the agent is on a call so new submissions show up.
  useEffect(() => {
    if (!authed || open) return;
    const t = setInterval(refresh, 10000);
    return () => clearInterval(t);
  }, [authed, open, refresh]);

  const nav = (
    <TopNav tag="Agent Console"
      right={authed ? (
        <button className="btn btn--ghost btn--sm" onClick={signOut}><Icon name="logout" size={15} /> Sign out</button>
      ) : undefined} />
  );

  if (!authed) {
    return (
      <>
        {nav}
        <main className="app-main" style={{ maxWidth: 440 }}>
          <div className="card card--kiosk app-stack">
            <div className="ds-h">
              <span className="ds-h__kicker">Agent Console</span>
              <h1 className="ds-h__title">Sign in</h1>
              <p className="ds-h__sub">Enter the agent PIN to see patient verifications.</p>
            </div>
            <div className="field">
              <label className="field__label" htmlFor="pin">Agent PIN</label>
              <div className="input input--kiosk">
                <span className="input__lead"><Icon name="lock" size={16} /></span>
                <input id="pin" type="password" inputMode="numeric" autoComplete="current-password" value={pin}
                  onChange={(e) => setPin(e.target.value)} onKeyDown={(e) => e.key === "Enter" && refresh()} />
              </div>
              {error && <span className="field__error">{error}</span>}
            </div>
            <button className="btn btn--cta btn--full" onClick={refresh}>Sign in <Icon name="arrow" size={15} sw={2.4} color="#fff" /></button>
          </div>
        </main>
      </>
    );
  }

  if (open) {
    const s = open.submission;
    const f = s.form;
    const failed = s.checks.filter((c) => !c.ok).length;
    return (
      <>
        {nav}
        <main className="app-main app-main--wide app-stack">
          <div>
            <button className="btn btn--ghost btn--sm" onClick={() => setOpen(null)}><Icon name="back" size={15} sw={2.2} /> All submissions</button>
          </div>
          <div className="card card--flush card--kiosk">
            <div className="card__head">
              <span className="avatar">{(f.license.firstName[0] ?? "") + (f.license.lastName[0] ?? "")}</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="card__title">{f.license.firstName} {f.license.lastName}</div>
                <div className="card__sub">Code <span className="t-mono">{s.code}</span> · {new Date(s.createdAt).toLocaleString()}</div>
              </div>
              <StatusChip status={s.status} />
            </div>
            <div className="card__body app-stack">
              {failed === 0 ? (
                <div className="verify-banner verify-banner--verified">
                  <span className="verify-banner__badge"><Icon name="check" size={13} sw={3} color="#fff" /></span>
                  All {s.checks.length} checks passed
                </div>
              ) : (
                <div className="verify-banner verify-banner--failed">
                  <span className="verify-banner__badge" style={{ background: "#F04438", color: "#fff" }}><Icon name="info" size={13} sw={3} /></span>
                  {failed} {failed === 1 ? "check needs" : "checks need"} review — go over {failed === 1 ? "it" : "them"} with the patient
                </div>
              )}
              <div className="list app-checks">
                {s.checks.map((c) => (
                  <div key={c.id} className="list-row">
                    <span className={`app-check-icon ${c.ok ? "is-ok" : "is-bad"}`}><Icon name={c.ok ? "check" : "info"} size={15} sw={2.4} /></span>
                    <div className="list-row__main">
                      <div className="list-row__title">{c.label}</div>
                      {!c.ok && c.detail && <div className="list-row__meta">{c.detail}</div>}
                    </div>
                  </div>
                ))}
              </div>
              <dl className="app-review">
                <dt>Phone</dt><dd>{fmtPhone(f.phone)}</dd>
                <dt>DOB entered</dt><dd>{f.dob}</dd>
                <dt>License</dt>
                <dd>
                  <span className="t-mono">{f.license.number}</span> ({f.license.state}) · DOB {f.license.dob} · exp {f.license.expiration}{" "}
                  <span className={`pill pill--sm ${f.license.scanned ? "pill--green" : "pill--outline"}`}>{f.license.scanned ? "Read from barcode" : "Typed by patient"}</span>
                </dd>
                <dt>Address</dt><dd>{f.license.street}, {f.license.city}, {f.license.state} {f.license.zip}</dd>
                <dt>Payment</dt>
                <dd>
                  {f.cashPay ? <span className="pill pill--gold">Self-pay</span> : (
                    <>
                      {f.insurance.plan === "__other__" ? <>Not listed: {f.insurance.otherPlanName}</> : f.insurance.plan} · Member{" "}
                      <span className="t-mono">{f.insurance.memberId}</span>{f.insurance.groupNumber && <> · Group <span className="t-mono">{f.insurance.groupNumber}</span></>}
                    </>
                  )}
                </dd>
              </dl>
            </div>
          </div>
          {Object.keys(open.images).length > 0 && (
            <div className="card card--kiosk">
              <div className="card__title">Photos</div>
              <div className="card__sub" style={{ marginBottom: 16 }}>Tap a photo to open it full size.</div>
              <div className="app-gallery">
                {Object.entries(open.images).map(([k, src]) => (
                  <figure key={k}>
                    <a href={src} target="_blank" rel="noreferrer">
                      {/* eslint-disable-next-line @next/next/no-img-element */}
                      <img src={src} alt={IMAGE_LABELS[k] ?? k} />
                    </a>
                    <figcaption className="t-caps">{IMAGE_LABELS[k] ?? k}</figcaption>
                  </figure>
                ))}
              </div>
            </div>
          )}
        </main>
      </>
    );
  }

  return (
    <>
      {nav}
      <main className="app-main app-main--wide app-stack">
        <div className="ds-h">
          <span className="ds-h__kicker">Agent Console</span>
          <h1 className="ds-h__title">Patient verifications</h1>
          <p className="ds-h__sub">Ask the patient for their 6-character code, or find them by phone. This list refreshes every 10 seconds.</p>
        </div>
        {!durable && (
          <div className="notice notice--info">
            <span className="icon"><Icon name="info" size={18} /></span>
            <div className="notice__text">Storage isn&apos;t connected yet, so some submissions may not show up here. Add Upstash Redis to this project in Vercel.</div>
          </div>
        )}
        {error && (
          <div className="verify-banner verify-banner--failed">
            <span className="verify-banner__badge" style={{ background: "#F04438", color: "#fff" }}><Icon name="close" size={13} sw={3} /></span>
            {error}
          </div>
        )}
        <div className="grid-tbl">
          <div className="grid-tbl__toolbar">
            <div className="searchbar searchbar--sq" style={{ flex: 1, maxWidth: 320 }}>
              <span className="searchbar__lead"><Icon name="search" size={15} /></span>
              <input placeholder="Code (ABC234) or phone" value={search} onChange={(e) => setSearch(e.target.value)}
                style={{ border: "none", outline: "none", background: "transparent", fontFamily: "var(--font-sans)", fontSize: 13.5, flex: 1, minWidth: 0 }}
                onKeyDown={(e) => {
                  if (e.key !== "Enter") return;
                  if (/^[A-Za-z0-9]{6}$/.test(search.trim()) && /[A-Za-z]/.test(search)) openCode(search);
                  else refresh();
                }} />
            </div>
            <div style={{ flex: 1 }} />
            <button className="grid-btn grid-btn--default" onClick={refresh}><Icon name="refresh" size={14} /> Refresh</button>
          </div>
          <div className="app-table-scroll">
            <table>
              <thead>
                <tr><th>Patient</th><th>Code</th><th>Phone</th><th>Time</th><th>Status</th><th style={{ textAlign: "right" }}>Actions</th></tr>
              </thead>
              <tbody>
                {list.map((s) => (
                  <tr key={s.code} onClick={() => openCode(s.code)}>
                    <td>
                      <div className="grid-tbl__name">{s.form.license.firstName} {s.form.license.lastName}</div>
                      <div className="grid-tbl__sub">DOB {s.form.dob}</div>
                    </td>
                    <td><span className="t-mono">{s.code}</span></td>
                    <td>{fmtPhone(s.form.phone)}</td>
                    <td>{new Date(s.createdAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</td>
                    <td><StatusChip status={s.status} /></td>
                    <td><div className="grid-actions"><button className="grid-btn grid-btn--primary">Open</button></div></td>
                  </tr>
                ))}
                {!list.length && (
                  <tr><td colSpan={6} style={{ textAlign: "center", color: "var(--ink-3)", padding: 32, cursor: "default" }}>No submissions yet.</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </main>
    </>
  );
}
