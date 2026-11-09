/** Columns of numbers pasted from a spreadsheet or a CSV file, for a
 *  component's series inputs (std:plot's xs, ys, ys2, …).
 *
 *  Rows are lines; cells split on tabs, else commas or semicolons, else
 *  spaces. A first row with any non-numeric cell is a header, and a header
 *  like `λ (nm)` or `power [mW]` gives its column a unit. Empty or
 *  non-numeric cells are NaN: a gap in a plotted line. */
export type Columns = { headers: (string | null)[]; units: (string | null)[]; columns: number[][] };

const numeric = (cell: string) => cell.trim() !== "" && Number.isFinite(Number(cell.trim()));

export function parseColumns(text: string): Columns | null {
  const lines = text.replace(/\r/g, "").split("\n").filter((l) => l.trim() !== "");
  if (!lines.length) return null;
  const sep = lines.some((l) => l.includes("\t")) ? /\t/ : lines.some((l) => /[,;]/.test(l)) ? /[,;]/ : /\s+/;
  const rows = lines.map((l) => l.split(sep).map((c) => c.trim()));
  const header = rows[0].some((c) => c !== "" && !numeric(c)) ? rows.shift()! : null;
  if (!rows.length) return null;
  const width = Math.max(...rows.map((r) => r.length), header?.length ?? 0);
  const columns = Array.from({ length: width }, (_, i) => rows.map((r) => (numeric(r[i] ?? "") ? Number(r[i]) : NaN)));
  // Drop columns with no numbers at all (a label column).
  const keep = columns.map((c) => c.some((v) => !Number.isNaN(v)));
  const headers = Array.from({ length: width }, (_, i) => header?.[i] || null);
  const units = headers.map((h) => h?.match(/[([]\s*([^)\]]+?)\s*[)\]]\s*$/)?.[1] ?? null);
  return {
    headers: headers.filter((_, i) => keep[i]),
    units: units.filter((_, i) => keep[i]),
    columns: columns.filter((_, i) => keep[i]),
  };
}

/** A column as the series text the engine reads: `"1 2 NaN 4 nm"`. */
export function seriesText(values: number[], unit: string | null): string {
  return [...values.map((v) => (Number.isNaN(v) ? "NaN" : String(v))), ...(unit ? [unit] : [])].join(" ");
}

/** The edits that put `cols` into a component's inputs: columns in order
 *  into its series inputs (one column alone is y against its index), and
 *  headers into `x_label`/`y_label` where the component has them. */
export function columnEdits(cols: Columns, inputs: { field: string; kind: string | null; text?: string | null }[]): Record<string, string> {
  const series = inputs.filter((i) => i.kind === "series").map((i) => i.field);
  const names = new Set(inputs.map((i) => i.field));
  const out: Record<string, string> = {};
  const lists = cols.columns.length === 1 ? [cols.columns[0].map((_, i) => i), cols.columns[0]] : cols.columns;
  const units = cols.columns.length === 1 ? [null, cols.units[0]] : cols.units;
  const headers = cols.columns.length === 1 ? [null, cols.headers[0]] : cols.headers;
  lists.slice(0, series.length).forEach((values, i) => {
    out[`args.${series[i]}`] = seriesText(values, units[i]);
  });
  // Series inputs past the pasted columns are emptied, so old data goes.
  const filled = new Set(inputs.filter((i) => i.kind === "series" && i.text && i.text !== "series()").map((i) => i.field));
  series.slice(lists.length).filter((s) => filled.has(s)).forEach((s) => (out[`args.${s}`] = "=series()"));
  if (headers[0] && names.has("x_label")) out["args.x_label"] = headers[0];
  if (headers[1] && names.has("y_label")) out["args.y_label"] = headers[1];
  return out;
}
