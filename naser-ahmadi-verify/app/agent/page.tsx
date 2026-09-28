"use client";

import { useCallback, useEffect, useState } from "react";
import type { Submission } from "@/lib/types";

const STATUS: Record<Submission["status"], string> = {
  verified: "Verified",
  needs_review: "Needs review",
  cash_pay: "Cash pay",
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

  // Poll while the agent is on a call so new submissions show up.
  useEffect(() => {
    if (!authed || open) return;
    const t = setInterval(refresh, 10000);
    return () => clearInterval(t);
  }, [authed, open, refresh]);

  if (!authed) {
    return (
      <main className="shell">
        <section className="card">
          <h1>Agent sign in</h1>
          <label>
            Agent PIN
            <input type="password" value={pin} onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && refresh()} />
          </label>
          <button className="primary" onClick={refresh}>Sign in</button>
          {error && <p className="error">{error}</p>}
        </section>
      </main>
    );
  }

  if (open) {
    const s = open.submission;
    const f = s.form;
    return (
      <main className="shell wide">
        <button className="link" onClick={() => setOpen(null)}>← All submissions</button>
        <section className="card">
          <div className="row">
            <h1>{f.license.firstName} {f.license.lastName}</h1>
            <span className={`badge ${s.status}`}>{STATUS[s.status]}</span>
          </div>
          <p className="note">Code <b>{s.code}</b> · {new Date(s.createdAt).toLocaleString()}</p>
          <ul className="checks">
            {s.checks.map((c) => (
              <li key={c.id} className={c.ok ? "ok" : "bad"}>
                <span aria-hidden>{c.ok ? "✓" : "!"}</span>
                <div>{c.label}{!c.ok && c.detail && <small>{c.detail}</small>}</div>
              </li>
            ))}
          </ul>
          <dl className="review">
            <dt>Phone</dt><dd>{fmtPhone(f.phone)}</dd>
            <dt>DOB entered</dt><dd>{f.dob}</dd>
            <dt>License</dt>
            <dd>{f.license.number} ({f.license.state}) · DOB {f.license.dob} · exp {f.license.expiration}
              {f.license.scanned ? " · read from barcode" : " · typed by patient"}</dd>
            <dt>Address</dt><dd>{f.license.street}, {f.license.city}, {f.license.state} {f.license.zip}</dd>
            <dt>Payment</dt>
            <dd>
              {f.cashPay
                ? "Cash pay"
                : `${f.insurance.plan === "__other__" ? `Not listed: ${f.insurance.otherPlanName}` : f.insurance.plan} · Member ${f.insurance.memberId}${f.insurance.groupNumber ? ` · Group ${f.insurance.groupNumber}` : ""}`}
            </dd>
          </dl>
          <div className="gallery">
            {Object.entries(open.images).map(([k, src]) => (
              <figure key={k}>
                <a href={src} target="_blank" rel="noreferrer">
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img src={src} alt={IMAGE_LABELS[k] ?? k} />
                </a>
                <figcaption>{IMAGE_LABELS[k] ?? k}</figcaption>
              </figure>
            ))}
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="shell wide">
      <section className="card">
        <h1>Patient submissions</h1>
        {!durable && (
          <p className="warn">
            Storage isn&apos;t connected, so submissions may not show up here. Add Upstash Redis in Vercel (see README).
          </p>
        )}
        <div className="row">
          <input placeholder="Code (ABC234) or phone" value={search} onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== "Enter") return;
              if (/^[A-Za-z0-9]{6}$/.test(search.trim()) && /[A-Za-z]/.test(search)) openCode(search);
              else refresh();
            }} />
          <button className="secondary" onClick={refresh}>Refresh</button>
        </div>
        {error && <p className="error">{error}</p>}
        <table className="table">
          <thead>
            <tr><th>Time</th><th>Code</th><th>Patient</th><th>Phone</th><th>Status</th></tr>
          </thead>
          <tbody>
            {list.map((s) => (
              <tr key={s.code} onClick={() => openCode(s.code)}>
                <td>{new Date(s.createdAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</td>
                <td><b>{s.code}</b></td>
                <td>{s.form.license.firstName} {s.form.license.lastName}</td>
                <td>{fmtPhone(s.form.phone)}</td>
                <td><span className={`badge ${s.status}`}>{STATUS[s.status]}</span></td>
              </tr>
            ))}
            {!list.length && (
              <tr><td colSpan={5} className="empty">No submissions yet. This list refreshes every 10 seconds.</td></tr>
            )}
          </tbody>
        </table>
      </section>
    </main>
  );
}
