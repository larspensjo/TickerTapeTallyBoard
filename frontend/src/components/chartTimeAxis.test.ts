import { describe, expect, it, vi } from "vitest";
import {
  calendarSpineData,
  chartDate,
  formatIsoDate,
  isoFromTime,
  parseIsoDate,
  tickMarkFormatter,
} from "./chartTimeAxis";

vi.mock("lightweight-charts", () => ({
  TickMarkType: {
    Year: 0,
    Month: 1,
    DayOfMonth: 2,
    Time: 3,
    TimeWithSeconds: 4,
  },
}));

describe("calendarSpineData", () => {
  it("fills missing calendar days so the x-axis is linear over time", () => {
    const spine = calendarSpineData(
      [
        { time: "2025-09-19", value: 157.99 },
        { time: "2025-09-22", value: 208.43 },
      ],
      "2025-09-16",
    );

    expect(spine).toEqual([
      { time: "2025-09-16" },
      { time: "2025-09-17" },
      { time: "2025-09-18" },
      { time: "2025-09-19", value: 157.99 },
      { time: "2025-09-20" },
      { time: "2025-09-21" },
      { time: "2025-09-22", value: 208.43 },
    ]);
  });

  it("starts at the first data point when no range start is given", () => {
    const spine = calendarSpineData(
      [
        { time: "2025-09-19", value: 1 },
        { time: "2025-09-21", value: 2 },
      ],
      undefined,
    );

    expect(spine).toEqual([
      { time: "2025-09-19", value: 1 },
      { time: "2025-09-20" },
      { time: "2025-09-21", value: 2 },
    ]);
  });

  it("returns the data untouched when the range start is after the last point", () => {
    const data = [{ time: "2025-09-19", value: 1 }];

    expect(calendarSpineData(data, "2025-09-25")).toEqual(data);
  });

  it("returns the data untouched when there is nothing to span", () => {
    expect(calendarSpineData([], "2025-09-16")).toEqual([]);
  });

  it("spans a month boundary without dropping or duplicating a day", () => {
    const spine = calendarSpineData(
      [
        { time: "2025-01-30", value: 1 },
        { time: "2025-02-02", value: 2 },
      ],
      "2025-01-30",
    );

    expect(spine.map((point) => point.time)).toEqual([
      "2025-01-30",
      "2025-01-31",
      "2025-02-01",
      "2025-02-02",
    ]);
  });
});

describe("tickMarkFormatter", () => {
  it("formats day ticks with a month name so first-history labels are not ambiguous", () => {
    expect(tickMarkFormatter("2025-05-11", 2)).toBe("May 11");
  });

  it("leaves non-day tick types to the library default", () => {
    expect(tickMarkFormatter("2025-05-01", 1)).toBeNull();
  });
});

describe("date helpers", () => {
  it("round-trips an ISO date through the chart timestamp representation", () => {
    const timestamp = parseIsoDate("2026-03-01");

    expect(timestamp).not.toBeNull();
    expect(timestamp && formatIsoDate(timestamp)).toBe("2026-03-01");
  });

  it("rejects values that are not ISO calendar dates", () => {
    expect(parseIsoDate("2026-3-1")).toBeNull();
    expect(parseIsoDate("not-a-date")).toBeNull();
  });

  it("reads an ISO date back out of a chart time", () => {
    expect(isoFromTime("2026-03-01")).toBe("2026-03-01");
    expect(isoFromTime({ year: 2026, month: 3, day: 1 })).toBe("2026-03-01");
  });

  it("returns null for a chart time that is not a real date", () => {
    expect(chartDate("nonsense")).toBeNull();
  });
});
