import { describe, expect, it } from "vitest";
import { fillPlaceholders, placeholderNames } from "./placeholders";

describe("placeholderNames", () => {
  it("lists unique names in order of first use", () => {
    expect(placeholderNames("scp {{file}} {{host}}:{{file}}")).toEqual(["file", "host"]);
  });

  it("ignores everything that is not {{name}}", () => {
    const tooLong = `{{${"n".repeat(33)}}}`;
    expect(placeholderNames(`{{ x y }} {{}} {{a.b}} ${tooLong} \${HOME} {x}`)).toEqual([]);
  });
});

describe("fillPlaceholders", () => {
  it("replaces every occurrence literally, without quoting", () => {
    expect(fillPlaceholders("cp {{f}} {{f}}.bak && echo '{{ x }}'", { f: "a b;$(rm)" })).toBe(
      "cp a b;$(rm) a b;$(rm).bak && echo '{{ x }}'",
    );
  });

  it("does not resolve placeholders inside values", () => {
    expect(fillPlaceholders("{{a}}{{b}}", { a: "{{b}}", b: "x" })).toBe("{{b}}x");
  });

  it("fills missing values with an empty string", () => {
    expect(fillPlaceholders("x{{a}}y", {})).toBe("xy");
  });
});
