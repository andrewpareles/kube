const isRecord = (value: unknown): value is { [key: string]: unknown } => {
  return typeof value === "object" && value !== null && !Array.isArray(value);
};

export const findEdgegapWebhookRequestId = (value: unknown): string | null => {
  if (!isRecord(value)) {
    return null;
  }
  if (typeof value["request_id"] === "string") {
    return value["request_id"];
  }
  if (typeof value["requestId"] === "string") {
    return value["requestId"];
  }
  for (const childValue of Object.values(value)) {
    const requestId = findEdgegapWebhookRequestId(childValue);
    if (requestId) {
      return requestId;
    }
  }
  return null;
};
