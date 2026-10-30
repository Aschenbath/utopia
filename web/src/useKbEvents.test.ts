import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createInvalidationCoalescer,
  createKbInvalidations,
  OVERVIEW_SETTLE_MS,
  SETTLE_MS,
} from "./useKbEvents";

describe("a burst of events buys one refetch", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("the same key pushed many times inside the settle window invalidates once", () => {
    const seen: unknown[] = [];
    const { push } = createInvalidationCoalescer((key) => seen.push(key), 300);
    for (let i = 0; i < 12; i++) push(["graph"]);
    expect(seen).toEqual([]);
    vi.advanceTimersByTime(299);
    expect(seen).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(seen).toEqual([["graph"]]);
  });

  it("different keys in one burst each invalidate once, in arrival order", () => {
    const seen: unknown[] = [];
    const { push } = createInvalidationCoalescer((key) => seen.push(key), 300);
    push(["documents", "kb"], ["graph"]);
    push(["graph"]);
    push(["review", "kb"]);
    vi.advanceTimersByTime(300);
    expect(seen).toEqual([["documents", "kb"], ["graph"], ["review", "kb"]]);
  });

  it("a burst after the window is a new burst", () => {
    const seen: unknown[] = [];
    const { push } = createInvalidationCoalescer((key) => seen.push(key), 300);
    push(["graph"]);
    vi.advanceTimersByTime(300);
    push(["graph"]);
    vi.advanceTimersByTime(300);
    expect(seen).toEqual([["graph"], ["graph"]]);
  });

  it("a broader key covers a narrower queued key regardless of their arrival order", () => {
    for (const keys of [
      [["graph", "kb"], ["graph"]],
      [["graph"], ["graph", "kb"]],
    ]) {
      const seen: unknown[] = [];
      const { push, flush } = createInvalidationCoalescer((key) => seen.push(key), 300);
      for (const key of keys) push(key);
      flush();
      expect(seen).toEqual([["graph"]]);
    }
  });

  it("flushing on unmount delivers what was pending instead of dropping it", () => {
    const seen: unknown[] = [];
    const { push, flush } = createInvalidationCoalescer((key) => seen.push(key), 300);
    push(["pending", "kb"]);
    flush();
    expect(seen).toEqual([["pending", "kb"]]);
    vi.advanceTimersByTime(300);
    expect(seen).toEqual([["pending", "kb"]]);
  });
});

describe("the overview deadline and the fast keys", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const overview = ["graph", "kb", "overview"];
  const documents = ["documents", "kb"];

  it("a fast key queued just before the deadline still waits out its own settle window", () => {
    const seen: unknown[] = [];
    const { push, pushGraph } = createKbInvalidations((key) => seen.push(key), "kb");
    pushGraph();
    vi.advanceTimersByTime(SETTLE_MS);
    seen.length = 0;
    vi.advanceTimersByTime(OVERVIEW_SETTLE_MS - SETTLE_MS - 10);
    push(documents);
    vi.advanceTimersByTime(10);
    expect(seen).toEqual([overview]);
    vi.advanceTimersByTime(SETTLE_MS - 10 - 1);
    expect(seen).toEqual([overview]);
    vi.advanceTimersByTime(1);
    expect(seen).toEqual([overview, documents]);
  });

  it("the overview waits for its own deadline, not the fast window", () => {
    const seen: unknown[] = [];
    const { pushGraph } = createKbInvalidations((key) => seen.push(key), "kb");
    pushGraph();
    vi.advanceTimersByTime(SETTLE_MS);
    expect(seen).toEqual([["graph", "kb", "neighborhood"]]);
    vi.advanceTimersByTime(OVERVIEW_SETTLE_MS - SETTLE_MS);
    expect(seen).toEqual([["graph", "kb", "neighborhood"], overview]);
  });

  it("a reconnect's broader graph key covers the queued overview", () => {
    const seen: unknown[] = [];
    const { push, pushGraph, flushAll } = createKbInvalidations((key) => seen.push(key), "kb");
    pushGraph();
    push(["graph", "kb"]);
    flushAll();
    expect(seen).toEqual([["graph", "kb"]]);
  });

  it("flushing everything delivers both layers and leaves no timer behind", () => {
    const seen: unknown[] = [];
    const { pushGraph, flushAll } = createKbInvalidations((key) => seen.push(key), "kb");
    pushGraph();
    flushAll();
    expect(seen).toEqual([["graph", "kb", "neighborhood"], overview]);
    vi.advanceTimersByTime(OVERVIEW_SETTLE_MS);
    expect(seen).toEqual([["graph", "kb", "neighborhood"], overview]);
  });
});
