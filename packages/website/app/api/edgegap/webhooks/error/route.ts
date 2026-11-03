import { handleEdgegapWebhookPost } from "@/lib/edgegapApi";

export const runtime = "nodejs";

export async function POST(request: Request) {
  return handleEdgegapWebhookPost({
    request,
    lifecycleEvent: "error",
  });
}
