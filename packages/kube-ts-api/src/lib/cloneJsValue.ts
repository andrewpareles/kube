// This copies observable proxies into plain JSON-shaped data before we send it back to Rust.
export const cloneJsValue = (value: unknown): unknown => {
    if (value === null || typeof value !== "object") {
      return value;
    }

  
    if (Array.isArray(value)) {
      const copiedValues: unknown[] = [];
      for (const item of value) {
        copiedValues.push(cloneJsValue(item));
      }
      return copiedValues;
    }
  
    const copiedValueOfKey: { [key: string]: unknown } = {};
    for (const [key, nestedValue] of Object.entries(value)) {
      if (typeof nestedValue === "function") continue;
      copiedValueOfKey[key] = cloneJsValue(nestedValue);
    }
    return copiedValueOfKey;
  };
  
