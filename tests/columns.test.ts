import { describe, expect, test } from "bun:test";
import { columnEdits, parseColumns } from "../src-ui/model/columns";

const plot = [
  { field: "xs", kind: "series", text: "series(41, 0…10)" },
  { field: "ys", kind: "series", text: "series(41, -1…1)" },
  { field: "ys2", kind: "series", text: "1 2 3" },
  { field: "ys3", kind: "series", text: "series()" },
  { field: "x_label", kind: "str", text: "" },
  { field: "y_label", kind: "str", text: "" },
];

describe("pasted columns", () => {
  test("a spreadsheet's two columns with a header and units", () => {
    const cols = parseColumns("λ (nm)\tpower [mW]\n400\t1.5\n450\t\n500\t2\n")!;
    expect(cols.units).toEqual(["nm", "mW"]);
    expect(cols.columns[1].map(String)).toEqual(["1.5", "NaN", "2"]);
    expect(columnEdits(cols, plot)).toEqual({
      "args.xs": "400 450 500 nm",
      "args.ys": "1.5 NaN 2 mW",
      "args.ys2": "=series()",
      "args.x_label": "λ (nm)",
      "args.y_label": "power [mW]",
    });
  });
  test("CSV without a header; one column is y against its index", () => {
    expect(parseColumns("1,2\n3,4")!.columns).toEqual([[1, 3], [2, 4]]);
    expect(columnEdits(parseColumns("5\n6\n7")!, plot.slice(0, 2))).toEqual({ "args.xs": "0 1 2", "args.ys": "5 6 7" });
  });
  test("a label column is skipped; nothing numeric is nothing", () => {
    expect(parseColumns("name,x\na,1\nb,2")!.columns).toEqual([[1, 2]]);
    expect(parseColumns("  \n")).toBeNull();
  });
});
