import { Kiosk } from "./Kiosk";
import { acceptedPlans } from "@/lib/config";

export const dynamic = "force-dynamic";

// The agent texts this link to the caller, e.g. /?phone=3105550100, so the
// patient never has to type their number.
export default async function Page({ searchParams }: { searchParams: Promise<{ phone?: string }> }) {
  const { phone } = await searchParams;
  return <Kiosk plans={acceptedPlans()} initialPhone={phone ?? ""} />;
}
