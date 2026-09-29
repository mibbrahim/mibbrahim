// Pulls member ID, group number and the insurance company out of the text read
// (OCR) from an insurance card photo. Cards have no standard layout, so this is
// best effort: the patient checks and corrects everything on the confirm screen.

export type InsuranceGuess = {
  payer?: string; // insurer name as found on the card, e.g. "Aetna"
  plan?: string; // matching entry from the accepted-plans list
  memberId?: string;
  groupNumber?: string;
};

// Payer name as written on cards → words used to match the accepted-plans list.
const PAYERS: [RegExp, string, string[]][] = [
  [/\baetna\b/i, "Aetna", ["aetna"]],
  [/\banthem\b/i, "Anthem Blue Cross", ["anthem"]],
  [/blue\s*shield/i, "Blue Shield of California", ["blue shield"]],
  [/blue\s*cross/i, "Blue Cross", ["blue cross", "anthem"]],
  [/\bcigna\b/i, "Cigna", ["cigna"]],
  [/united\s*health|\buhc\b/i, "UnitedHealthcare", ["united"]],
  [/\bmedicare\b/i, "Medicare", ["medicare"]],
  [/medi-?cal/i, "Medi-Cal", ["medi-cal", "medical"]],
  [/\bkaiser\b/i, "Kaiser Permanente", ["kaiser"]],
  [/\bhumana\b/i, "Humana", ["humana"]],
  [/health\s*net/i, "Health Net", ["health net"]],
  [/molina/i, "Molina", ["molina"]],
  [/oscar/i, "Oscar", ["oscar"]],
];

const ID_RE = /(?:member\s*(?:id|#|no\.?|number)?|subscriber\s*(?:id|#)?|identification\s*(?:no\.?|number|#)?|\bid\s*(?:#|no\.?|number)?)\s*[:#.]?\s*([A-Z0-9][A-Z0-9 -]{4,20}[A-Z0-9])/i;
const GROUP_RE = /(?:group|grp)\s*(?:#|no\.?|number|id)?\s*[:#.]?\s*([A-Z0-9][A-Z0-9-]{2,15})/i;

function clean(s: string): string {
  return s.replace(/[\s-]+/g, "").toUpperCase();
}

export function parseInsuranceText(text: string, accepted: string[]): InsuranceGuess {
  const guess: InsuranceGuess = {};
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);

  for (const line of lines) {
    if (!guess.groupNumber) {
      const g = GROUP_RE.exec(line);
      if (g) guess.groupNumber = clean(g[1]);
    }
    if (!guess.memberId && !/group|grp|rx\s*bin|pcn/i.test(line.split(/id/i)[0] ?? "")) {
      const m = ID_RE.exec(line);
      // A member ID has at least a few digits.
      if (m && /\d{4,}/.test(clean(m[1]))) guess.memberId = clean(m[1]).slice(0, 20);
    }
  }
  // Fallback: a token that looks like an ID (letters then 6+ digits), e.g. "W123456789".
  if (!guess.memberId) {
    const t = /\b([A-Z]{1,4}\d{6,14})\b/.exec(text.toUpperCase());
    if (t && t[1] !== guess.groupNumber) guess.memberId = t[1];
  }

  for (const [re, name, keys] of PAYERS) {
    if (!re.test(text)) continue;
    guess.payer = name;
    guess.plan = accepted.find((p) => keys.some((k) => p.toLowerCase().includes(k)));
    break;
  }
  return guess;
}
