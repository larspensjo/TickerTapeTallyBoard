const AMOUNT_PATTERN = /^[+-]?(?:(?:\d+(?:\.\d*)?)|(?:\.\d+))$/;
const COMMA_DECIMAL_PATTERN = /^[+-]?(?:(?:\d+,\d{1,2})|(?:,\d{1,2}))$/;

function normalizeCommaDecimal(amount: string): string {
  return amount.replace(",", ".");
}

export function normalizeRebalanceAmount(amount: string | null): string | null {
  if (amount === null) {
    return null;
  }

  const trimmed = amount.trim();
  if (trimmed === "") {
    return null;
  }

  if (trimmed.includes(",")) {
    if (!COMMA_DECIMAL_PATTERN.test(trimmed)) {
      return null;
    }

    return normalizeCommaDecimal(trimmed);
  }

  const normalized = trimmed;
  return AMOUNT_PATTERN.test(normalized) ? normalized : null;
}
