import assert from "node:assert/strict";
import test from "node:test";
import { findEdgegapWebhookRequestId } from "../packages/website/lib/edgegapWebhookPayload.ts";

test("findEdgegapWebhookRequestId reads a top-level request_id", () => {
  const requestId = findEdgegapWebhookRequestId({
    request_id: "req-top-level",
  });

  assert.equal(requestId, "req-top-level");
});

test("findEdgegapWebhookRequestId unwraps a nested requestId", () => {
  const requestId = findEdgegapWebhookRequestId({
    data: {
      deployment: {
        requestId: "req-nested",
      },
    },
  });

  assert.equal(requestId, "req-nested");
});
