import { Kiosk } from "./Kiosk";
import { acceptedPlans, practiceName } from "@/lib/config";

export const dynamic = "force-dynamic";

export default async function Page({ searchParams }: { searchParams: Promise<{ phone?: string }> }) {
  const { phone } = await searchParams;
  return <Kiosk practice={practiceName()} plans={acceptedPlans()} initialPhone={phone ?? ""} />;
}
