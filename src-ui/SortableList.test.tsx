import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SortableList, reorderWithinSubset } from "./SortableList";

afterEach(() => cleanup());

test("reorderWithinSubset pins non-subset ids and reorders subset slots", () => {
  // full [a, b(pinned), c]; enabled subset reordered to [c, a]
  expect(reorderWithinSubset(["a", "b", "c"], ["c", "a"])).toEqual([
    "c",
    "b",
    "a",
  ]);
  // unchanged subset order leaves the full list untouched
  expect(reorderWithinSubset(["a", "b", "c"], ["a", "c"])).toEqual([
    "a",
    "b",
    "c",
  ]);
  // a fully-enabled list reorders directly
  expect(reorderWithinSubset(["a", "b"], ["b", "a"])).toEqual(["b", "a"]);
});

test("renders each item with a reorder handle", () => {
  render(
    <SortableList
      items={[{ id: "a" }, { id: "b" }]}
      onReorder={vi.fn()}
      ariaLabel="Items"
      renderItem={(item, handle) => (
        <div>
          {handle}
          <span>{item.id}</span>
        </div>
      )}
    />,
  );

  expect(screen.getByLabelText("Items")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Reorder" })).toHaveLength(2);
  expect(screen.getByText("a")).toBeInTheDocument();
  expect(screen.getByText("b")).toBeInTheDocument();
});
