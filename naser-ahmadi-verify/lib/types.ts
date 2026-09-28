export type IntakeForm = {
  phone: string;
  dob: string; // YYYY-MM-DD, typed by the patient
  license: {
    firstName: string;
    lastName: string;
    number: string;
    dob: string; // YYYY-MM-DD, as printed on the license
    expiration: string; // YYYY-MM-DD
    street: string;
    city: string;
    state: string;
    zip: string;
    scanned: boolean; // true when filled from the barcode
  };
  insurance: {
    plan: string; // one of the accepted plans, or OTHER_PLAN
    otherPlanName: string;
    memberId: string;
    groupNumber: string;
  };
  cashPay: boolean;
  images: {
    licenseFront?: string; // data URL (JPEG)
    licenseBack?: string;
    insuranceFront?: string;
    insuranceBack?: string;
  };
};

export const OTHER_PLAN = "__other__";

export type Check = {
  id: string;
  label: string;
  ok: boolean;
  detail?: string;
};

export type Submission = {
  code: string;
  createdAt: string;
  form: Omit<IntakeForm, "images">;
  imageKeys: string[];
  checks: Check[];
  status: "verified" | "needs_review" | "cash_pay";
};
