const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** "2026-10-07" → "7 Oct". Calendar dates are formatted without a time zone. */
export function formatShortDate(isoDate: string): string {
  const match = isoDate.match(/^\d{4}-(\d{2})-(\d{2})$/);
  const month = match ? MONTHS[Number(match[1]) - 1] : undefined;
  if (!match || !month) {
    return isoDate;
  }

  return `${Number(match[2])} ${month}`;
}
