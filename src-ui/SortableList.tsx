import type { ReactNode } from "react";
import {
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";

/**
 * Reusable single-column drag-to-reorder list. The consumer renders each item
 * and places the supplied drag handle wherever it likes. `onReorder` fires with
 * the full reordered id list only when the order actually changes.
 */
export function SortableList<T extends { id: string }>({
  items,
  onReorder,
  renderItem,
  ariaLabel,
  className,
}: {
  items: T[];
  onReorder: (orderedIds: string[]) => void;
  renderItem: (item: T, handle: ReactNode) => ReactNode;
  ariaLabel?: string;
  className?: string;
}) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  function handleDragEnd(event: DragEndEvent) {
    const next = computeReorder(
      items.map((item) => item.id),
      String(event.active.id),
      event.over ? String(event.over.id) : null,
    );
    if (next) {
      onReorder(next);
    }
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={items.map((item) => item.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className={className} role="list" aria-label={ariaLabel}>
          {items.map((item) => (
            <SortableRow key={item.id} id={item.id}>
              {(handle) => renderItem(item, handle)}
            </SortableRow>
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );
}

function SortableRow({
  id,
  children,
}: {
  id: string;
  children: (handle: ReactNode) => ReactNode;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 2 : undefined,
  };
  const handle = (
    <button
      type="button"
      className="drag-handle"
      aria-label="Reorder"
      {...attributes}
      {...listeners}
    >
      <GripVertical size={15} />
    </button>
  );
  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`sortable-item${isDragging ? " dragging" : ""}`}
      role="listitem"
    >
      {children(handle)}
    </div>
  );
}

/**
 * Compute the new id order for a drag, or null when nothing should change
 * (no drop target, dropped on itself, or an unknown id).
 */
export function computeReorder(
  ids: string[],
  activeId: string,
  overId: string | null,
): string[] | null {
  if (!overId || activeId === overId) {
    return null;
  }
  const from = ids.indexOf(activeId);
  const to = ids.indexOf(overId);
  if (from < 0 || to < 0) {
    return null;
  }
  return arrayMove(ids, from, to);
}

/**
 * Map a reordered subset back onto the full id list. Members of `subsetOrderedIds`
 * fill the positions the subset occupied in `fullIds` (in their new order), while
 * ids outside the subset stay pinned to their absolute positions. Used by the
 * tray, which drags only the enabled usage cards but must persist the full order.
 */
export function reorderWithinSubset(
  fullIds: string[],
  subsetOrderedIds: string[],
): string[] {
  const subset = new Set(subsetOrderedIds);
  const queue = [...subsetOrderedIds];
  return fullIds.map((id) => (subset.has(id) ? (queue.shift() ?? id) : id));
}
