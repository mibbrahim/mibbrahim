# Patient Form — verification link

A mobile-first page (Next.js / React) that the agent texts to a patient who is on the phone with them. The agent's link carries the patient's mobile number (`/?phone=3105550100`; `?mobile=` and `?p=` also work), and it is shown pre-filled on the confirm screen as "From the text message we sent you".

1. **Scan your driver's license** (front + back). The PDF417 barcode on the back is read on the phone and fills in name, date of birth, address, license number and expiration.
2. **Scan your insurance card**. The card text is read on the phone (OCR, tesseract.js) to find the insurer, member ID and group number. Patients without insurance tap **"I'll pay cash"**.
3. **Are these details correct?** Everything is pre-filled and editable; the patient ticks the confirmation box and submits.
4. The patient gets a 6-character code to read to the agent.

Patients who'd rather not use the link tap **"I'd rather do this over the phone"**.

UI components come from the PracticeEHR design system (https://design-components-ks.netlify.app/). `app/ds/tokens.css` and `app/ds/components.css` are vendored verbatim; `app/app.css` only handles layout.

The agent opens **`/agent`**, signs in with the agent PIN, and sees each submission with the photos and a pass/fail list:

| Check | Rule |
|---|---|
| Date of birth | Read from the license and plausible |
| California license | Issuing state is CA and the number is 1 letter + 7 digits |
| Not expired | Expiration date is today or later |
| California address | ZIP code is in California (state found from ZIP), street and city present |
| Insurance accepted | Plan is on the practice's accepted list |
| Photos | License front and insurance front uploaded |

Status is **Verified**, **Needs review** (the agent goes over anything marked `!` with the patient) or **Cash pay**.

## Configure (Vercel → Project → Settings → Environment Variables)

| Variable | Purpose |
|---|---|
| `ACCEPTED_PLANS` | Comma-separated accepted plans. **Replace the placeholder list with Naser Ahmadi's real list.** |
| `AGENT_PIN` | PIN agents type on `/agent`. Required. |
| `KV_REST_API_URL` / `KV_REST_API_TOKEN` | Storage. Add **Upstash Redis** from Vercel → Storage / Marketplace and connect it to the project; these are set automatically. Without it, submissions won't reliably reach `/agent` on Vercel. |
| `RETENTION_DAYS` | Days to keep submissions and photos (default 30). |
| `DEMO_AUTOFILL` | **Prototype.** Fills any field the scans could not read with sample data, labelled "Sample data". On unless set to `false` — turn it off before real patients use the form. |

## Deploy

In Vercel: **Add New → Project → import this repo**, set **Root Directory** to `naser-ahmadi-verify`, add the env vars above, deploy.

Or from the CLI:

```bash
cd naser-ahmadi-verify
npx vercel --prod
```

## Develop

```bash
npm install
AGENT_PIN=1234 npm run dev   # http://localhost:3000 and /agent
npm test                     # validation + barcode parsing tests
```

## Not built yet

- $0.01 card authorization to confirm a valid payment method (needs a Stripe account)
- Looking up existing patients by phone number (needs EHR access)
- Real-time insurance eligibility check (needs a clearinghouse such as Availity/pVerify)
- SMS sending: for now the agent texts the link manually

## Compliance note

This collects PHI (ID and insurance card photos). Before using it with real patients, make sure the hosting and storage providers are covered by a BAA (Vercel offers HIPAA/BAA on paid plans; Upstash offers BAA on its Enterprise plan), and restrict `AGENT_PIN` to staff.
