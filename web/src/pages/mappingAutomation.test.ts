import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { ConceptMapping } from "../api";

import { S } from "../i18n";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";

vi.mock("../kb", () => ({ useKb: () => ({ kb: { id: "kb1" } }) }));
import { Mappings } from "./Mappings";

function render(mapping: ConceptMapping) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(["mappings", "kb1", "all", "", 0], {
    items: [mapping], total: 1, counts: { proposed: 0, confirmed: 1, rejected: 0 },
  });
  return renderToStaticMarkup(createElement(QueryClientProvider, {
    client, children: createElement(Mappings),
  }));
}

const automatic: ConceptMapping = {
  id: "m1", concept_id: "e1", concept_name: "Revenue", source: "Warehouse",
  table_name: "orders", expr: "SUM(amount)", sql: null, unit: null,
  summary: null, derived: false, status: "confirmed", decided_by: null,
  confidence: 0.9, rationale: "The schema defines the amount column",
  auto_confirmed_at: "2026-10-30T00:00:00Z",
};

describe("mapping decision origin and evidence", () => {
  it("marks an automatic definition and offers every way to answer it", () => {
    const html = render(automatic);
    expect(html).toContain(S.mapping.autoConfirmed);
    expect(html).toContain(S.mapping.approve);
    expect(html).toContain(S.mapping.reject);
    expect(html).toContain(S.mapping.revertAutomatic);
  });

  it("shows the model's evidence", () => {
    const html = render(automatic);
    expect(html).toContain(S.mapping.confidence(0.9));
    expect(html).toContain(S.mapping.rationale(automatic.rationale!));
  });

  it("drops the mark and the revert once a person has decided", () => {
    const html = render({ ...automatic, decided_by: "editor", auto_confirmed_at: null });
    expect(html).not.toContain(S.mapping.autoConfirmed);
    expect(html).not.toContain(S.mapping.revertAutomatic);
  });

  it("does not call a confirmed row automatic only because nobody is recorded on it", () => {
    const html = render({ ...automatic, auto_confirmed_at: null });
    expect(html).not.toContain(S.mapping.autoConfirmed);
    expect(html).not.toContain(S.mapping.revertAutomatic);
  });

  it("shows no score when the model gave none", () => {
    const html = render({
      ...automatic, status: "proposed", confidence: null, rationale: null, auto_confirmed_at: null,
    });
    expect(html).not.toContain("90%");
  });

  it("words the score in both languages", () => {
    expect(en.mapping.confidence(0.8)).toBe("Confidence: 80%");
    expect(zh.mapping.confidence(0.8)).toBe("置信度：80%");
  });
});
