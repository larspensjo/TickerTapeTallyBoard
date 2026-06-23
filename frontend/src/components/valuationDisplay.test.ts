import { describe, expect, it } from "vitest";
import {
  availabilityNumber,
  availabilitySortValues,
  formatGroupedNumber,
  isAvailable,
  parseFiniteNumber,
  signedTone,
} from "./valuationDisplay";

describe("isAvailable", () => {
  it("returns true for an available value", () => {
    expect(isAvailable({ status: "available", value: "123" })).toBe(true);
  });

  it("returns false for an unavailable value", () => {
    expect(isAvailable({ status: "unavailable", reasons: ["missing"] })).toBe(
      false,
    );
  });

  it("returns false for undefined", () => {
    expect(isAvailable(undefined)).toBe(false);
  });
});

describe("availabilityNumber", () => {
  it("returns the parsed number for an available finite value", () => {
    expect(availabilityNumber({ status: "available", value: "1234.5" })).toBe(
      1234.5,
    );
    expect(availabilityNumber({ status: "available", value: "-42" })).toBe(-42);
    expect(availabilityNumber({ status: "available", value: "0" })).toBe(0);
  });

  it("returns -Infinity for unavailable", () => {
    expect(availabilityNumber({ status: "unavailable", reasons: ["x"] })).toBe(
      Number.NEGATIVE_INFINITY,
    );
  });

  it("returns -Infinity for an available non-finite value", () => {
    expect(
      availabilityNumber({ status: "available", value: "not-a-number" }),
    ).toBe(Number.NEGATIVE_INFINITY);
  });
});

describe("availabilitySortValues", () => {
  it("sorts smaller values before larger", () => {
    const small = { status: "available" as const, value: "10" };
    const large = { status: "available" as const, value: "100" };
    expect(availabilitySortValues(small, large)).toBeLessThan(0);
    expect(availabilitySortValues(large, small)).toBeGreaterThan(0);
  });

  it("sorts unavailable values below available ones", () => {
    const unavailable = {
      status: "unavailable" as const,
      reasons: ["missing"],
    };
    const available = { status: "available" as const, value: "1" };
    expect(availabilitySortValues(unavailable, available)).toBeLessThan(0);
    expect(availabilitySortValues(available, unavailable)).toBeGreaterThan(0);
  });

  it("returns 0 for equal values", () => {
    const a = { status: "available" as const, value: "5" };
    const b = { status: "available" as const, value: "5" };
    expect(availabilitySortValues(a, b)).toBe(0);
  });
});

describe("signedTone", () => {
  it("classifies sign", () => {
    expect(signedTone("1")).toBe("up");
    expect(signedTone("-1")).toBe("down");
    expect(signedTone("0")).toBe("flat");
    expect(signedTone("nope")).toBe("flat");
  });

  it("returns flat for non-finite values", () => {
    expect(signedTone("Infinity")).toBe("flat");
    expect(signedTone("-Infinity")).toBe("flat");
  });
});

describe("formatGroupedNumber", () => {
  it("groups thousands with commas and passes through non-numerics", () => {
    expect(formatGroupedNumber("1234567.89")).toBe("1,234,567.89");
    expect(formatGroupedNumber("-1234.5")).toBe("-1,234.5");
    expect(formatGroupedNumber("n/a")).toBe("n/a");
  });

  it("groups integer strings without a fractional part", () => {
    expect(formatGroupedNumber("1000")).toBe("1,000");
    expect(formatGroupedNumber("999")).toBe("999");
  });

  it("accepts number inputs", () => {
    expect(formatGroupedNumber(1234)).toBe("1,234");
    expect(formatGroupedNumber(-5000)).toBe("-5,000");
  });
});

describe("parseFiniteNumber", () => {
  it("parses finite strings and numbers", () => {
    expect(parseFiniteNumber("12.5")).toBe(12.5);
    expect(parseFiniteNumber(7)).toBe(7);
    expect(parseFiniteNumber("-3.14")).toBe(-3.14);
  });

  it("returns null for non-numeric strings and infinite values", () => {
    expect(parseFiniteNumber("nope")).toBeNull();
    expect(parseFiniteNumber(Number.POSITIVE_INFINITY)).toBeNull();
    expect(parseFiniteNumber(Number.NEGATIVE_INFINITY)).toBeNull();
  });

  it("returns 0 for empty string (Number('') === 0, which is finite)", () => {
    expect(parseFiniteNumber("")).toBe(0);
  });
});
