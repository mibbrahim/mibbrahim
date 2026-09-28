import { NextResponse } from "next/server";
import { checkAgentPin } from "@/lib/auth";
import { getImage, getSubmission, hasDurableStore, listSubmissions } from "@/lib/store";
import { normalizePhone } from "@/lib/validate";

// Agent-only. ?code=ABC123 returns one submission with images;
// ?phone=... filters the recent list; no params returns the recent list.
export async function GET(req: Request) {
  const auth = checkAgentPin(req);
  if (auth === "unset") return NextResponse.json({ error: "AGENT_PIN is not set on the server." }, { status: 503 });
  if (auth === "denied") return NextResponse.json({ error: "Wrong PIN" }, { status: 401 });

  const params = new URL(req.url).searchParams;
  const code = params.get("code")?.trim().toUpperCase();

  if (code) {
    const sub = await getSubmission(code);
    if (!sub) return NextResponse.json({ error: "Not found" }, { status: 404 });
    const images: Record<string, string> = {};
    for (const k of sub.imageKeys) {
      const img = await getImage(code, k);
      if (img) images[k] = img;
    }
    return NextResponse.json({ submission: sub, images });
  }

  const phone = normalizePhone(params.get("phone") ?? "");
  let subs = await listSubmissions(100);
  if (phone) subs = subs.filter((s) => s.form.phone === phone);
  return NextResponse.json({ submissions: subs, durable: hasDurableStore });
}
