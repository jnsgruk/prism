import { useCallback, useRef, useState } from "react";

export type MentionType = "file" | "person" | "team";

export type MentionItem = {
  id: string;
  name: string;
  type: MentionType;
};

/** Data attributes used to identify pill elements in the contentEditable div. */
export const PILL_ID_ATTR = "data-mention-id";
export const PILL_TYPE_ATTR = "data-mention-type";

/** @deprecated Kept for backward compatibility with legacy pills. */
const LEGACY_PILL_ATTR = "data-mention-path";

/**
 * Read the text content immediately before the cursor inside a contentEditable
 * element and detect an active @mention query.
 */
const getTextBeforeCursor = (): string | null => {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return null;
  const range = sel.getRangeAt(0);
  if (!range.collapsed) return null;

  const node = range.startContainer;
  if (node.nodeType !== Node.TEXT_NODE) return null;
  return (node.textContent ?? "").slice(0, range.startOffset);
};

const findMentionQuery = (textBefore: string): string | null => {
  const atIndex = textBefore.lastIndexOf("@");
  if (atIndex === -1) return null;
  if (atIndex > 0 && !/\s/.test(textBefore[atIndex - 1]!)) return null;
  const query = textBefore.slice(atIndex + 1);
  if (/\s/.test(query)) return null;
  return query;
};

const PILL_COLORS: Record<MentionType, string> = {
  file: "bg-primary/10 text-primary",
  person: "bg-violet-500/10 text-violet-700 dark:text-violet-400",
  team: "bg-amber-500/10 text-amber-700 dark:text-amber-400",
};

/**
 * Create a pill DOM element for a mentioned entity.
 */
export const createPillElement = (
  id: string,
  displayName: string,
  type: MentionType,
): HTMLSpanElement => {
  const pill = document.createElement("span");
  pill.setAttribute(PILL_ID_ATTR, id);
  pill.setAttribute(PILL_TYPE_ATTR, type);
  pill.setAttribute("contenteditable", "false");
  pill.className = `inline-flex items-center gap-1 rounded-md ${PILL_COLORS[type]} px-1.5 py-0.5 text-xs align-baseline mx-0.5 select-none`;
  pill.textContent = displayName;

  const removeBtn = document.createElement("span");
  removeBtn.className = "cursor-pointer ml-0.5 hover:opacity-70";
  removeBtn.textContent = "×";
  removeBtn.setAttribute("role", "button");
  removeBtn.setAttribute("aria-label", `Remove ${displayName}`);
  removeBtn.addEventListener("mousedown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    pill.remove();
  });
  pill.appendChild(removeBtn);

  return pill;
};

/**
 * Extract plain text and mentioned entities from a contentEditable div.
 * Pills are replaced with their display names in the text output (their
 * structured data goes into the mentions array).
 */
export const extractContent = (el: HTMLElement): { text: string; mentions: MentionItem[] } => {
  const mentions: MentionItem[] = [];
  let text = "";

  const walk = (node: Node): void => {
    if (node.nodeType === Node.TEXT_NODE) {
      text += node.textContent ?? "";
      return;
    }
    if (node instanceof HTMLElement) {
      // New-style pill: data-mention-id + data-mention-type
      const pillId = node.getAttribute(PILL_ID_ATTR);
      if (pillId) {
        const name = node.firstChild?.textContent ?? pillId;
        const pillType = (node.getAttribute(PILL_TYPE_ATTR) ?? "file") as MentionType;
        mentions.push({ id: pillId, name, type: pillType });
        text += name;
        return;
      }
      // Legacy pill: data-mention-path (treat as file)
      const legacyPath = node.getAttribute(LEGACY_PILL_ATTR);
      if (legacyPath) {
        const name = node.firstChild?.textContent ?? legacyPath;
        mentions.push({ id: legacyPath, name, type: "file" });
        text += name;
        return;
      }
      // Handle <br> as newline
      if (node.tagName === "BR") {
        text += "\n";
        return;
      }
      // Handle block elements (div wraps lines in contentEditable)
      if (node.tagName === "DIV" && text.length > 0 && !text.endsWith("\n")) {
        text += "\n";
      }
    }
    for (const child of node.childNodes) {
      walk(child);
    }
  };

  walk(el);
  return { text: text.trim(), mentions };
};

type MentionPickerState = {
  mentionQuery: string | null;
  mentionActive: boolean;
  detectMention: () => void;
  insertPill: (id: string, name: string, type: MentionType, editorEl: HTMLElement) => void;
  closeMention: () => void;
  editorRef: React.RefObject<HTMLDivElement | null>;
};

export const useMentionPicker = (): MentionPickerState => {
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const editorRef = useRef<HTMLDivElement | null>(null);

  const detectMention = useCallback(() => {
    const textBefore = getTextBeforeCursor();
    if (textBefore === null) {
      setMentionQuery(null);
      return;
    }
    setMentionQuery(findMentionQuery(textBefore));
  }, []);

  const insertPill = useCallback(
    (id: string, name: string, type: MentionType, editorEl: HTMLElement) => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return;

      const range = sel.getRangeAt(0);
      const node = range.startContainer;
      if (node.nodeType !== Node.TEXT_NODE) return;

      const textContent = node.textContent ?? "";
      const cursorOffset = range.startOffset;
      const textBefore = textContent.slice(0, cursorOffset);
      const atIndex = textBefore.lastIndexOf("@");
      if (atIndex === -1) return;

      // Split the text node: [before @] [pill] [after cursor]
      const before = textContent.slice(0, atIndex);
      const after = textContent.slice(cursorOffset);

      const parent = node.parentNode;
      if (!parent) return;

      const beforeNode = document.createTextNode(before);
      const pill = createPillElement(id, name, type);
      // Add a trailing space so the cursor has somewhere to land
      const afterNode = document.createTextNode(after || "\u00A0");

      parent.insertBefore(beforeNode, node);
      parent.insertBefore(pill, node);
      parent.insertBefore(afterNode, node);
      parent.removeChild(node);

      // Place cursor after the pill (in the afterNode)
      const newRange = document.createRange();
      newRange.setStart(afterNode, after ? 0 : 1);
      newRange.collapse(true);
      sel.removeAllRanges();
      sel.addRange(newRange);

      setMentionQuery(null);

      // Trigger input event so React picks up the change
      editorEl.dispatchEvent(new Event("input", { bubbles: true }));
    },
    [],
  );

  const closeMention = useCallback(() => {
    setMentionQuery(null);
  }, []);

  return {
    mentionQuery,
    mentionActive: mentionQuery !== null,
    detectMention,
    insertPill,
    closeMention,
    editorRef,
  };
};
