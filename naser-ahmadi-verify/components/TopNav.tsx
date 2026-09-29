import type { ReactNode } from "react";
import { Icon } from "./Icon";

// Design-system Top Navigation. The step progress sits inside the same bar.
export function TopNav({ tag, practice, center, right }: {
  tag: string; practice?: string; center?: ReactNode; right?: ReactNode;
}) {
  return (
    <header className="topnav topnav--tall app-topnav">
      <div className="topnav__brand">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img className="topnav__logo" src="/logo.png" alt="PracticeEHR" />
        <div className="topnav__tag app-tag">{tag}</div>
      </div>
      {practice && <div className="topnav__divider app-hide-sm" />}
      {practice && <span className="topnav__eyebrow app-hide-sm">{practice}</span>}
      <div className="topnav__spacer" />
      {center}
      {center && <div className="topnav__spacer" />}
      <div className="topnav__right">
        {right ?? (
          <span className="status status--blue" aria-label="Secure"><Icon name="lock" size={13} /><span className="app-hide-xs">Secure</span></span>
        )}
      </div>
    </header>
  );
}

export function Stepper({ steps, current }: { steps: string[]; current: number }) {
  return (
    <div className="stepper" aria-label={`Step ${current + 1} of ${steps.length}`}>
      {steps.map((label, i) => (
        <div key={label} style={{ display: "contents" }}>
          {i > 0 && <div className={`stepper__conn ${i <= current ? "is-done" : ""}`} />}
          <div className={`stepper__step ${i < current ? "is-done" : i === current ? "is-active" : ""}`}>
            <span className="stepper__circle">
              {i < current ? <Icon name="check" size={12} sw={3} color="#fff" /> : i + 1}
            </span>
            <span className="stepper__label">{label}</span>
          </div>
        </div>
      ))}
    </div>
  );
}
