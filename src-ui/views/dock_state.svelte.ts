// Dock state views react to (panel-primitives.md §4: a layout changes only
// when a person chooses): which groups have hidden their tab bar, and a
// dock-driven drag in progress (the grab handle starts one).
export const dockState = $state({ hidden: [] as string[], dragging: false });
