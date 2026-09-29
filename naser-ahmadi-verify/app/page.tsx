import { Kiosk } from "./Kiosk";
import { acceptedPlans } from "@/lib/config";

export const dynamic = "force-dynamic";

// The agent texts this link to the caller, e.g. /?phone=3105550100, so the
// patient never has to type their number.
export default async function Page({ searchParams }: { searchParams: Promise<{ phone?: string }> }) {
  const { phone } = await searchParams;
  // Prototype: fill anything the scans can't read with sample data. Set
  // DEMO_AUTOFILL=false before real patients use the form.
  const demo = process.env.DEMO_AUTOFILL !== "false";
  return <Kiosk plans={acceptedPlans()} initialPhone={phone ?? ""} demo={demo} />;
}
