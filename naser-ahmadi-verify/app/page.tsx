import { Kiosk } from "./Kiosk";
import { acceptedPlans } from "@/lib/config";

export const dynamic = "force-dynamic";

type Params = { phone?: string; mobile?: string; p?: string };

// The agent texts this link to the caller's mobile, e.g. /?phone=3105550100,
// so the number is filled in for them.
export default async function Page({ searchParams }: { searchParams: Promise<Params> }) {
  const { phone, mobile, p } = await searchParams;
  // Prototype: fill anything the scans can't read with sample data. Set
  // DEMO_AUTOFILL=false before real patients use the form.
  const demo = process.env.DEMO_AUTOFILL !== "false";
  return <Kiosk plans={acceptedPlans()} initialPhone={phone ?? mobile ?? p ?? ""} demo={demo} />;
}
