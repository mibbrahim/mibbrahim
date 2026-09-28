// Parses the PDF417 barcode on the back of a US driver's license (AAMVA
// standard). California licenses carry name, DOB, address, license number
// and expiration in this barcode.

export type LicenseData = {
  firstName?: string;
  lastName?: string;
  licenseNumber?: string;
  dob?: string; // YYYY-MM-DD
  expiration?: string; // YYYY-MM-DD
  street?: string;
  city?: string;
  state?: string;
  zip?: string;
};

// AAMVA dates are MMDDCCYY in the US; some older cards use CCYYMMDD.
function parseDate(raw: string | undefined): string | undefined {
  if (!raw || !/^\d{8}$/.test(raw)) return undefined;
  const asUs = { m: raw.slice(0, 2), d: raw.slice(2, 4), y: raw.slice(4, 8) };
  const asIso = { y: raw.slice(0, 4), m: raw.slice(4, 6), d: raw.slice(6, 8) };
  const pick = Number(asUs.m) >= 1 && Number(asUs.m) <= 12 && Number(asUs.y) > 1900 ? asUs : asIso;
  return `${pick.y}-${pick.m}-${pick.d}`;
}

function title(s: string | undefined): string | undefined {
  if (!s) return undefined;
  return s
    .toLowerCase()
    .replace(/\b[a-z]/g, (c) => c.toUpperCase())
    .trim();
}

export function isAamva(text: string): boolean {
  return /ANSI\s*\d{6}|AAMVA/.test(text) && /\bDAQ|DAQ/.test(text);
}

export function parseAamva(text: string): LicenseData {
  const fields: Record<string, string> = {};
  // Some readers escape control characters as "<LF>", "<CR>", "<RS>".
  text = text.replace(/<(?:LF|CR|RS|GS)>/g, "\n");
  for (const line of text.split(/[\r\n\x1e]+/)) {
    // The header line ends with the subfile type ("DL" or "ID") glued to the
    // first element, e.g. "ANSI 6360...DLDAQD1234567".
    const cleaned = /ANSI/.test(line) ? line.replace(/^.*?(?:DL|ID)(?=D[A-Z]{2})/, "") : line;
    const m = /^(D[A-Z]{2})(.*)$/.exec(cleaned);
    if (m && !(m[1] in fields)) fields[m[1]] = m[2].trim();
  }

  let first = fields.DAC ?? fields.DCT;
  let last = fields.DCS ?? fields.DAB;
  if (!first && !last && fields.DAA) {
    const parts = fields.DAA.split(/[,$]/);
    last = parts[0];
    first = parts[1];
  }

  const zipRaw = fields.DAK?.replace(/\s/g, "");
  return {
    firstName: title(first?.split(/[,\s]/)[0]),
    lastName: title(last),
    licenseNumber: fields.DAQ?.replace(/\s/g, "").toUpperCase(),
    dob: parseDate(fields.DBB),
    expiration: parseDate(fields.DBA),
    street: title(fields.DAG),
    city: title(fields.DAI),
    state: fields.DAJ?.toUpperCase(),
    zip: zipRaw ? zipRaw.slice(0, 5) : undefined,
  };
}
