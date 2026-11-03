import assert from "node:assert/strict";
import test from "node:test";
import { getPlanIdForLookupKey } from "../packages/website/lib/stripePlan.ts";

test("getPlanIdForLookupKey knows only the plans in pricing.ts", () => {
  assert.equal(getPlanIdForLookupKey("pro"), "pro");
  assert.equal(getPlanIdForLookupKey("max"), "max");
  assert.equal(getPlanIdForLookupKey("autumn_starter"), undefined);
  assert.equal(getPlanIdForLookupKey(null), undefined);
});
