import { describe, expect, it } from "vite-plus/test";

import { createPillElement, extractContent, PILL_ID_ATTR, PILL_TYPE_ATTR } from "./use-mention-picker";

describe("createPillElement", () => {
  it("creates pill with correct data attributes for file type", () => {
    const pill = createPillElement("src/main.rs", "main.rs", "file");
    expect(pill.getAttribute(PILL_ID_ATTR)).toBe("src/main.rs");
    expect(pill.getAttribute(PILL_TYPE_ATTR)).toBe("file");
    expect(pill.getAttribute("contenteditable")).toBe("false");
  });

  it("creates pill with correct data attributes for person type", () => {
    const pill = createPillElement("abc-123", "Alice", "person");
    expect(pill.getAttribute(PILL_ID_ATTR)).toBe("abc-123");
    expect(pill.getAttribute(PILL_TYPE_ATTR)).toBe("person");
  });

  it("creates pill with correct data attributes for team type", () => {
    const pill = createPillElement("team-456", "Platform", "team");
    expect(pill.getAttribute(PILL_ID_ATTR)).toBe("team-456");
    expect(pill.getAttribute(PILL_TYPE_ATTR)).toBe("team");
  });

  it("contains display name text", () => {
    const pill = createPillElement("id", "Display Name", "person");
    expect(pill.textContent).toContain("Display Name");
  });

  it("contains remove button with aria-label", () => {
    const pill = createPillElement("id", "Alice", "person");
    const removeBtn = pill.querySelector("[role=button]");
    expect(removeBtn).not.toBeNull();
    expect(removeBtn!.getAttribute("aria-label")).toBe("Remove Alice");
  });

  it("remove button click removes pill from DOM", () => {
    const container = document.createElement("div");
    const pill = createPillElement("id", "Alice", "person");
    container.appendChild(pill);
    expect(container.children.length).toBe(1);

    const removeBtn = pill.querySelector<HTMLElement>("[role=button]");
    if (!removeBtn) throw new Error("Expected remove button");
    removeBtn.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(container.children.length).toBe(0);
  });

  it("applies type-specific CSS classes", () => {
    const filePill = createPillElement("f", "f.txt", "file");
    expect(filePill.className).toContain("bg-primary");

    const personPill = createPillElement("p", "Alice", "person");
    expect(personPill.className).toContain("bg-violet");

    const teamPill = createPillElement("t", "Core", "team");
    expect(teamPill.className).toContain("bg-amber");
  });
});

describe("extractContent", () => {
  const makeEditor = (): HTMLDivElement => {
    const el = document.createElement("div");
    el.setAttribute("contenteditable", "true");
    return el;
  };

  it("returns empty text and no mentions for empty editor", () => {
    const el = makeEditor();
    const result = extractContent(el);
    expect(result.text).toBe("");
    expect(result.mentions).toEqual([]);
  });

  it("extracts plain text with no mentions", () => {
    const el = makeEditor();
    el.textContent = "Hello world";
    const result = extractContent(el);
    expect(result.text).toBe("Hello world");
    expect(result.mentions).toEqual([]);
  });

  it("extracts a single file mention pill", () => {
    const el = makeEditor();
    el.appendChild(document.createTextNode("Check "));
    el.appendChild(createPillElement("src/main.rs", "main.rs", "file"));
    const result = extractContent(el);
    expect(result.text).toBe("Check main.rs");
    expect(result.mentions).toEqual([{ id: "src/main.rs", name: "main.rs", type: "file" }]);
  });

  it("extracts a single person mention pill", () => {
    const el = makeEditor();
    el.appendChild(document.createTextNode("Ask "));
    el.appendChild(createPillElement("abc-123", "Alice", "person"));
    const result = extractContent(el);
    expect(result.text).toBe("Ask Alice");
    expect(result.mentions).toEqual([{ id: "abc-123", name: "Alice", type: "person" }]);
  });

  it("extracts a single team mention pill", () => {
    const el = makeEditor();
    el.appendChild(createPillElement("team-456", "Platform", "team"));
    const result = extractContent(el);
    expect(result.text).toBe("Platform");
    expect(result.mentions).toEqual([{ id: "team-456", name: "Platform", type: "team" }]);
  });

  it("extracts mixed pills in correct order", () => {
    const el = makeEditor();
    el.appendChild(document.createTextNode("Compare "));
    el.appendChild(createPillElement("abc-123", "Alice", "person"));
    el.appendChild(document.createTextNode(" in "));
    el.appendChild(createPillElement("team-456", "Platform", "team"));
    el.appendChild(document.createTextNode(" from "));
    el.appendChild(createPillElement("src/main.rs", "main.rs", "file"));
    const result = extractContent(el);
    expect(result.text).toBe("Compare Alice in Platform from main.rs");
    expect(result.mentions).toHaveLength(3);
    expect(result.mentions[0]!.type).toBe("person");
    expect(result.mentions[1]!.type).toBe("team");
    expect(result.mentions[2]!.type).toBe("file");
  });

  it("handles br elements as newlines alongside pills", () => {
    const el = makeEditor();
    el.appendChild(document.createTextNode("Line 1"));
    el.appendChild(document.createElement("br"));
    el.appendChild(createPillElement("id", "Alice", "person"));
    const result = extractContent(el);
    expect(result.text).toBe("Line 1\nAlice");
  });

  it("falls back to file type for legacy pills with data-mention-path", () => {
    const el = makeEditor();
    const legacyPill = document.createElement("span");
    legacyPill.setAttribute("data-mention-path", "old/file.txt");
    legacyPill.textContent = "file.txt";
    el.appendChild(legacyPill);
    const result = extractContent(el);
    expect(result.mentions).toEqual([{ id: "old/file.txt", name: "file.txt", type: "file" }]);
  });
});
