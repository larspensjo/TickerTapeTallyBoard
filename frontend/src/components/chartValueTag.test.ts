import type { SeriesAttachedParameter, Time } from "lightweight-charts";
import { describe, expect, it } from "vitest";
import { fixedTextValueTag } from "./chartValueTag";

function attachedTo(priceToCoordinate: (price: number) => number | null) {
  const tag = fixedTextValueTag({
    value: 112.125,
    text: "112.12",
    color: "#e0b15e",
    textColor: "black",
  });
  tag.attached?.({
    series: { priceToCoordinate },
  } as unknown as SeriesAttachedParameter<Time>);
  return tag;
}

describe("fixedTextValueTag", () => {
  it("draws the supplied text verbatim instead of rounding the value half-up", () => {
    const [view] = attachedTo((price) => 400 - price).priceAxisViews?.() ?? [];

    expect(view?.text()).toBe("112.12");
    expect(view?.backColor()).toBe("#e0b15e");
    expect(view?.textColor()).toBe("black");
  });

  it("sits at the value's level and follows the scale on each update", () => {
    let offset = 400;
    const tag = attachedTo((price) => offset - price);
    const [view] = tag.priceAxisViews?.() ?? [];

    expect(view?.coordinate()).toBe(400 - 112.125);
    expect(view?.visible?.()).toBe(true);

    offset = 300;
    tag.updateAllViews?.();
    expect(view?.coordinate()).toBe(300 - 112.125);
  });

  it("hides while the value has no coordinate on the scale", () => {
    const [view] = attachedTo(() => null).priceAxisViews?.() ?? [];

    expect(view?.visible?.()).toBe(false);
  });

  it("hides once detached from its series", () => {
    const tag = attachedTo((price) => 400 - price);
    tag.detached?.();
    const [view] = tag.priceAxisViews?.() ?? [];

    expect(view?.visible?.()).toBe(false);
  });
});
