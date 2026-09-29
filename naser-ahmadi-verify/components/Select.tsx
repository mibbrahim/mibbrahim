"use client";

import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

// Design-system SmartSelect (field size): hairline button, floating menu,
// soft-blue selected option with a trailing check.
export function Select({ value, options, placeholder, onChange, id }: {
  value: string; options: { value: string; label: string }[]; placeholder: string;
  onChange: (v: string) => void; id?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", esc); };
  }, [open]);

  const current = options.find((o) => o.value === value);
  return (
    <div ref={ref} className={`select select--field ${open ? "is-open" : ""}`}>
      <button id={id} type="button" className="select__btn" aria-haspopup="listbox" aria-expanded={open}
        onClick={() => setOpen((o) => !o)}>
        <span style={{ flex: 1, textAlign: "left", color: current ? undefined : "var(--ink-4)" }}>
          {current?.label ?? placeholder}
        </span>
        <Icon className="select__chev" name="chevD" size={14} sw={2} />
      </button>
      {open && <div className="select__menu" role="listbox">
        {options.map((o) => (
          <button key={o.value} type="button" role="option" aria-selected={o.value === value}
            className={`select__opt ${o.value === value ? "is-sel" : ""}`}
            onClick={() => { onChange(o.value); setOpen(false); }}>
            {o.label} {o.value === value && <Icon name="check" size={13} sw={3} />}
          </button>
        ))}
      </div>}
    </div>
  );
}
