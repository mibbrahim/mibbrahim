# Naser Ahmadi – Patient Verification Link

A kiosk-style link you text to patients while they're on the phone with an agent. The patient:

1. Confirms their phone number (pre-filled if the link has `?phone=3105550100`)
2. Enters their date of birth
3. Photographs their **California driver's license** (front + back). The barcode on the back is read automatically and fills in name, license #, DOB, expiration and address.
4. Picks their insurance plan from the practice's accepted list and photographs the card. If the plan isn't listed, they're offered **cash pay**.
5. Gets a 6-character code to read to the agent.

Patients who'd rather not use the link can tap **"I'd rather do this over the phone"**, and the agent collects the details on the call.

The agent opens **`/agent`**, signs in with the agent PIN, and sees each submission with the photos and a pass/fail list:

| Check | Rule |
|---|---|
| Date of birth | Typed DOB matches the DOB on the license |
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
| `PRACTICE_NAME` | Name shown to patients (default "Dr. Naser Ahmadi's Office"). |
| `KV_REST_API_URL` / `KV_REST_API_TOKEN` | Storage. Add **Upstash Redis** from Vercel → Storage / Marketplace and connect it to the project; these are set automatically. Without it, submissions won't reliably reach `/agent` on Vercel. |
| `RETENTION_DAYS` | Days to keep submissions and photos (default 30). |

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
