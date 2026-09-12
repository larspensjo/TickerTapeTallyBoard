import { describe, expect, it } from "vitest";
import { type DataVersion, versionToken } from "./dataVersion";

const version: DataVersion = {
  data_revision: "1757682000123:7",
  valuation_date: "2026-09-12",
  prices_refreshing: false,
};

describe("versionToken", () => {
  it("combines the revision and the server's date", () => {
    expect(versionToken(version)).toBe("1757682000123:7@2026-09-12");
  });

  it("changes when either part changes", () => {
    expect(versionToken({ ...version, data_revision: "x:8" })).not.toBe(
      versionToken(version),
    );
    expect(versionToken({ ...version, valuation_date: "2026-09-13" })).not.toBe(
      versionToken(version),
    );
  });
});
