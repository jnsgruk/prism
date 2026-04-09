import { useCallback, useRef, useState } from "react";

export type MentionedFile = {
  path: string;
  name: string;
};

/** Data attribute used to identify pill elements in the contentEditable div. */
export const PILL_ATTR = "data-mention-path";

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

/**
 * Create a pill DOM element for a mentioned file.
 */
export const createPillElement = (path: string, displayName: string): HTMLSpanElement => {
  const pill = document.createElement("span");
  pill.setAttribute(PILL_ATTR, path);
  pill.setAttribute("contenteditable", "false");
  pill.className =
    "inline-flex items-center gap-1 rounded-md bg-primary/10 px-1.5 py-0.5 text-xs text-primary align-baseline mx-0.5 select-none";
  pill.textContent = displayName;

  const removeBtn = document.createElement("span");
  removeBtn.className = "cursor-pointer ml-0.5 hover:text-primary/70";
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
 * Extract plain text and mentioned file paths from a contentEditable div.
 * Pills are replaced with empty string in the text output (their paths go
 * into the mentionedFiles array).
 */
export const extractContent = (
  el: HTMLElement,
): { text: string; mentionedFiles: MentionedFile[] } => {
  const mentionedFiles: MentionedFile[] = [];
  let text = "";

  const walk = (node: Node): void => {
    if (node.nodeType === Node.TEXT_NODE) {
      text += node.textContent ?? "";
      return;
    }
    if (node instanceof HTMLElement) {
      const path = node.getAttribute(PILL_ATTR);
      if (path) {
        const name = node.firstChild?.textContent ?? path;
        mentionedFiles.push({ path, name });
        // Include the filename inline so the message text reads naturally
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
  return { text: text.trim(), mentionedFiles };
};

type MentionPickerState = {
  mentionQuery: string | null;
  mentionActive: boolean;
  detectMention: () => void;
  insertPill: (path: string, name: string, editorEl: HTMLElement) => void;
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

  const insertPill = useCallback((path: string, name: string, editorEl: HTMLElement) => {
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
    const pill = createPillElement(path, name);
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
  }, []);

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
