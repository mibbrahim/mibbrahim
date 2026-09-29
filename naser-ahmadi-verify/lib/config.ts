// Server-side configuration. Values come from environment variables so the
// practice can change them in Vercel without a code change.

// Placeholder list. Replace with Naser Ahmadi's real accepted plans, either
// here or with the ACCEPTED_PLANS environment variable.
const DEFAULT_PLANS = [
  "Aetna PPO",
  "Anthem Blue Cross PPO",
  "Blue Shield of California PPO",
  "Cigna PPO",
  "UnitedHealthcare PPO",
  "Medicare Part B",
];

export function acceptedPlans(): string[] {
  const raw = process.env.ACCEPTED_PLANS;
  if (!raw) return DEFAULT_PLANS;
  const plans = raw
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean);
  return plans.length ? plans : DEFAULT_PLANS;
}

export function retentionSeconds(): number {
  const days = Number(process.env.RETENTION_DAYS ?? "30");
  return Math.max(1, Number.isFinite(days) ? days : 30) * 24 * 60 * 60;
}

export function agentPin(): string | undefined {
  return process.env.AGENT_PIN?.trim() || undefined;
}
