import type { LicenseData } from "./aamva.ts";

// Pulls details out of the text read (OCR) from the FRONT of a California
// driver's license. CA cards print short labels next to each value:
//   DL D1234567   EXP 04/12/2029   LN DOE   FN JANE   DOB 04/12/1985
//   123 MAIN ST / LOS ANGELES, CA 90012
// OCR is noisy, so this is best effort; the barcode on the back is preferred.

function isoDate(s: string | undefined): string | undefined {
  const m = s && /(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})/.exec(s);
  if (!m) return undefined;
  const [mm, dd, yyyy] = [Number(m[1]), Number(m[2]), m[3]];
  if (mm < 1 || mm > 12 || dd < 1 || dd > 31) return undefined;
  return `${yyyy}-${String(mm).padStart(2, "0")}-${String(dd).padStart(2, "0")}`;
}

function title(s: string | undefined): string | undefined {
  return s?.toLowerCase().replace(/\b[a-z]/g, (c) => c.toUpperCase()).trim() || undefined;
}

export function parseLicenseText(text: string): LicenseData {
  const t = text.toUpperCase().replace(/[|]/g, "I");
  const out: LicenseData = {};

  const dl = /\bDL\s*[:#]?\s*([A-Z]\s?\d{7})\b/.exec(t) ?? /\b([A-Z]\d{7})\b/.exec(t);
  if (dl) out.licenseNumber = dl[1].replace(/\s/g, "");

  out.expiration = isoDate(/\bEXP\w*\s*[:.]?\s*([\d/.-]{8,10})/.exec(t)?.[1]);
  out.dob = isoDate(/\bDOB\s*[:.]?\s*([\d/.-]{8,10})/.exec(t)?.[1]);

  const ln = /\bLN\s*[:.]?\s*([A-Z][A-Z' -]{1,30})/.exec(t);
  const fn = /\bFN\s*[:.]?\s*([A-Z][A-Z' -]{1,30})/.exec(t);
  if (ln) out.lastName = title(ln[1].split(/\s{2,}|\n/)[0]);
  if (fn) out.firstName = title(fn[1].split(/\s+/)[0]);

  // "CITY, CA 90012" — the street is usually the line above it.
  const lines = t.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  for (let i = 0; i < lines.length; i++) {
    const m = /^([A-Z][A-Z .'-]+?),?\s+(CA|[A-Z]{2})\s+(\d{5})(?:-\d{4})?\b/.exec(lines[i]);
    if (!m) continue;
    out.city = title(m[1]);
    out.state = m[2];
    out.zip = m[3];
    const street = lines[i - 1];
    if (street && /^\d+\s+\S/.test(street)) out.street = title(street);
    break;
  }
  return out;
}
