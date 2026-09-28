import { timingSafeEqual } from "node:crypto";
import { agentPin } from "./config.ts";

export function checkAgentPin(req: Request): "ok" | "unset" | "denied" {
  const pin = agentPin();
  if (!pin) return "unset";
  const given = req.headers.get("x-agent-pin") ?? "";
  const a = Buffer.from(given);
  const b = Buffer.from(pin);
  return a.length === b.length && timingSafeEqual(a, b) ? "ok" : "denied";
}
