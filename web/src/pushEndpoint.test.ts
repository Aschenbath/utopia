import { describe, expect, it } from "vitest";

import { pushEndpoint } from "./pushEndpoint";

describe("pushEndpoint", () => {
  it("api 来源推到 /ingest", () => {
    expect(pushEndpoint("http://localhost:1516", "abc", "api")).toBe(
      "http://localhost:1516/api/v1/sources/abc/ingest",
    );
  });

  it("statements 来源推到 /statements，不是 /ingest", () => {
    expect(pushEndpoint("http://localhost:1516", "abc", "statements")).toBe(
      "http://localhost:1516/api/v1/sources/abc/statements",
    );
  });
});
