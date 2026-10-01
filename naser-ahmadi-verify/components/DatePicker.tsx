"use client";

import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

// Design-system Date Picker: input-styled trigger that opens the .cal month
// calendar. Tapping the calendar title zooms out to months, then years, so a
// date of birth decades back is three taps away.

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const YEARS_PER_PAGE = 12;

type View = "days" | "months" | "years";

function parse(iso: string): { y: number; m: number; d: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  return m ? { y: Number(m[1]), m: Number(m[2]) - 1, d: Number(m[3]) } : null;
}
const iso = (y: number, m: number, d: number) => `${y}-${String(m + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
export const formatDate = (v: string) => {
  const p = parse(v);
  return p ? `${MONTHS[p.m]} ${String(p.d).padStart(2, "0")}, ${p.y}` : "";
};

// ISO "1985-04-12" → "04/12/1985", the format patients type.
const toTyped = (v: string) => {
  const p = parse(v);
  return p ? `${String(p.m + 1).padStart(2, "0")}/${String(p.d).padStart(2, "0")}/${p.y}` : "";
};

// Digits → "MM", "MM/DD", "MM/DD/YYYY" as the patient types.
function mask(raw: string): string {
  const d = raw.replace(/\D/g, "").slice(0, 8);
  if (d.length > 4) return `${d.slice(0, 2)}/${d.slice(2, 4)}/${d.slice(4)}`;
  if (d.length > 2) return `${d.slice(0, 2)}/${d.slice(2)}`;
  return d;
}

export function DatePicker({ id, value, onChange, placeholder = "MM/DD/YYYY", min, max, startYear, autoComplete }: {
  id?: string; value: string; onChange: (v: string) => void; placeholder?: string;
  min?: string; max?: string; // ISO dates; days outside are disabled
  startYear?: number; // year to open on when there's no value (e.g. ~35 years ago for a date of birth)
  autoComplete?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  // Typed text, kept separate from `value` so a half-typed date isn't wiped.
  const [text, setText] = useState(() => toTyped(value));
  const [typeError, setTypeError] = useState("");
  const emitted = useRef(value);

  // Sync when the value changes from outside (calendar pick, auto-fill from a scan).
  useEffect(() => {
    if (value !== emitted.current) {
      emitted.current = value;
      setText(toTyped(value));
      setTypeError("");
    }
  }, [value]);

  function emit(v: string) {
    emitted.current = v;
    if (v !== value) onChange(v);
  }

  function onType(raw: string) {
    const t = mask(raw);
    setText(t);
    setTypeError("");
    const m = /^(\d{2})\/(\d{2})\/(\d{4})$/.exec(t);
    if (!m) return emit("");
    const [mm, dd, yyyy] = [Number(m[1]), Number(m[2]), Number(m[3])];
    const d = new Date(Date.UTC(yyyy, mm - 1, dd));
    const real = d.getUTCFullYear() === yyyy && d.getUTCMonth() === mm - 1 && d.getUTCDate() === dd;
    const v = iso(yyyy, mm - 1, dd);
    if (!real) { setTypeError("That date doesn't exist — check the month and day."); return emit(""); }
    if (outOfRange(v)) {
      setTypeError(max && v > max ? "That date is in the future." : "Please check the year.");
      return emit("");
    }
    emit(v);
  }

  function onBlurInput() {
    if (text && text.length < 10) setTypeError("Enter the full date as MM/DD/YYYY.");
  }
  const [view, setView] = useState<View>("days");
  const today = new Date();
  const init = parse(value) ?? { y: startYear ?? today.getFullYear(), m: startYear ? 0 : today.getMonth(), d: 1 };
  const [cursor, setCursor] = useState({ y: init.y, m: init.m });

  // Re-centre on the current value whenever the popover opens.
  function toggle() {
    if (!open) {
      const p = parse(value);
      setCursor(p ? { y: p.y, m: p.m } : { y: init.y, m: init.m });
      setView(value || !startYear ? "days" : "years");
    }
    setOpen((o) => !o);
  }

  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", esc); };
  }, [open]);

  const outOfRange = (d: string) => (min !== undefined && d < min) || (max !== undefined && d > max);
  const minY = parse(min ?? "")?.y ?? 1900;
  const maxY = parse(max ?? "")?.y ?? today.getFullYear() + 20;
  const sel = parse(value);

  function shift(dir: -1 | 1) {
    if (view === "days") {
      const m = cursor.m + dir;
      setCursor({ y: cursor.y + Math.floor(m / 12), m: ((m % 12) + 12) % 12 });
    } else if (view === "months") setCursor({ ...cursor, y: cursor.y + dir });
    else setCursor({ ...cursor, y: cursor.y + dir * YEARS_PER_PAGE });
  }

  const pageStart = cursor.y - (((cursor.y - 1900) % YEARS_PER_PAGE) + YEARS_PER_PAGE) % YEARS_PER_PAGE;
  const title = view === "days" ? `${MONTHS_LONG[cursor.m]} ${cursor.y}` : view === "months" ? `${cursor.y}` : `${pageStart} – ${pageStart + YEARS_PER_PAGE - 1}`;

  const firstDow = new Date(cursor.y, cursor.m, 1).getDay();
  const daysIn = new Date(cursor.y, cursor.m + 1, 0).getDate();

  return (
    <div ref={ref} className={`datepicker app-datepicker ${open ? "is-open" : ""}`}>
      <div className={`datepicker__trigger app-dp-field ${typeError ? "is-error" : ""}`}>
        <button type="button" className="app-dp-btn icon" aria-label="Open calendar" aria-haspopup="dialog" aria-expanded={open} onClick={toggle}>
          <Icon name="cal" size={16} />
        </button>
        <input id={id} className="app-dp-input" type="text" inputMode="numeric" autoComplete={autoComplete} placeholder={placeholder}
          aria-invalid={Boolean(typeError)} value={text} maxLength={10}
          onChange={(e) => onType(e.target.value)} onBlur={onBlurInput} />
        <button type="button" className="app-dp-btn chev" aria-label="Open calendar" tabIndex={-1} onClick={toggle}>
          <Icon name="chevD" size={14} sw={2} />
        </button>
      </div>
      {typeError && <span className="field__error" role="alert">{typeError}</span>}
      {open && (
        <div className="datepicker__pop" role="dialog" aria-label="Choose a date">
          <div className="cal">
            <div className="cal__head">
              <button className="cal__nav" type="button" aria-label="Previous" onClick={() => shift(-1)}>‹</button>
              <button type="button" className="cal__title app-cal__title" aria-label="Change view"
                onClick={() => setView(view === "days" ? "months" : "years")} disabled={view === "years"}>
                {title} {view !== "years" && <Icon name="chevD" size={12} sw={2.2} />}
              </button>
              <button className="cal__nav" type="button" aria-label="Next" onClick={() => shift(1)}>›</button>
            </div>

            {view === "days" && (
              <>
                <div className="cal__dow">{["S", "M", "T", "W", "T", "F", "S"].map((d, i) => <span key={i}>{d}</span>)}</div>
                <div className="cal__grid">
                  {Array.from({ length: firstDow }, (_, i) => <span key={`b${i}`} />)}
                  {Array.from({ length: daysIn }, (_, i) => {
                    const d = i + 1, v = iso(cursor.y, cursor.m, d), off = outOfRange(v);
                    const isSel = sel?.y === cursor.y && sel.m === cursor.m && sel.d === d;
                    return (
                      <button key={d} type="button" disabled={off} aria-pressed={isSel}
                        className={`cal__day ${isSel ? "is-sel" : ""} ${off ? "is-disabled" : ""}`}
                        onClick={() => { onChange(v); setOpen(false); }}>{d}</button>
                    );
                  })}
                </div>
              </>
            )}

            {view === "months" && (
              <div className="cal__grid app-cal__wide">
                {MONTHS.map((m, i) => (
                  <button key={m} type="button" className={`cal__day ${sel?.y === cursor.y && sel.m === i ? "is-sel" : ""}`}
                    onClick={() => { setCursor({ ...cursor, m: i }); setView("days"); }}>{m}</button>
                ))}
              </div>
            )}

            {view === "years" && (
              <div className="cal__grid app-cal__wide">
                {Array.from({ length: YEARS_PER_PAGE }, (_, i) => {
                  const y = pageStart + i, off = y < minY || y > maxY;
                  return (
                    <button key={y} type="button" disabled={off} className={`cal__day ${sel?.y === y ? "is-sel" : ""} ${off ? "is-disabled" : ""}`}
                      onClick={() => { setCursor({ ...cursor, y }); setView("months"); }}>{y}</button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
