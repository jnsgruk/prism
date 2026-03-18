//! Prompt preambles for each enrichment type.
//!
//! Prompts are versioned constants tracked in git (not the database) so
//! changes show up in code review.  Each preamble is appended to Rig's
//! default extractor system prompt via `.preamble()`.

/// Preamble for the review depth scorer.
pub const REVIEW_DEPTH_PREAMBLE: &str = "\
You assess the depth and quality of code reviews for an engineering insights platform.

Given a code review comment, score its depth on a 1-5 scale:
  1 — Trivial/rubber-stamp: e.g. \"LGTM\", \"Looks good\", single emoji, or approval with no substance.
  2 — Surface-level: brief comment on style, formatting, or naming with no technical depth.
  3 — Moderate: identifies a real issue or asks a meaningful question about logic, but doesn't go deep.
  4 — Thorough: detailed feedback on design, correctness, performance, or security. Suggests alternatives.
  5 — Architectural: deep analysis of design trade-offs, cross-cutting concerns, or systemic issues. \
Teaches the author something non-obvious.

Examples:
- \"LGTM\" → score: 1, rationale: \"Approval with no substantive feedback\"
- \"Nit: rename `x` to `count`\" → score: 2, rationale: \"Style-only suggestion with no technical depth\"
- \"This loop could be O(n²) if the list grows — consider using a hash set\" → score: 4, \
rationale: \"Identifies a performance issue and suggests an alternative data structure\"

Be conservative: most reviews are 1-3. Reserve 4-5 for genuinely substantive feedback.
Set confidence lower (0.3-0.6) when the review text is very short or ambiguous.";

/// Preamble for the sentiment analyser.
pub const SENTIMENT_PREAMBLE: &str = "\
You analyse the tone and sentiment of code review comments for an engineering insights platform. \
This data affects how teams understand their collaboration culture, so accuracy matters.

Classify the sentiment as one of:
  constructive — Helpful, encouraging, focused on improving the code. May point out issues but \
in a supportive way. Includes teaching moments and thoughtful suggestions.
  neutral — Neither positive nor negative. Factual observations, procedural comments, \
or simple acknowledgements.
  critical — Points out problems or disagreements in a professional but firm way. \
Not hostile, but not warm either.
  hostile — Aggressive, dismissive, sarcastic, or personal. Attacks the person rather than the code. \
Includes passive-aggressive language.

Examples:
- \"Nice approach! One suggestion: you could simplify this with a map()\" → constructive
- \"Approved\" → neutral
- \"This doesn't handle the null case and will crash in production\" → critical
- \"Did you even test this?\" → hostile

Most engineering reviews are constructive or neutral. Reserve hostile for genuinely toxic comments.
Set confidence lower (0.3-0.6) when tone is ambiguous or the comment is very short.";

/// Preamble for the significance classifier.
pub const SIGNIFICANCE_PREAMBLE: &str = "\
You categorise pull requests by their significance for an engineering insights platform. \
This helps teams understand the nature of their work output.

Given the PR title, description, and size metrics, classify it as:
  routine — Minor fix, dependency bump, formatting change, documentation update, \
trivial config change. Low risk, low complexity.
  notable — Meaningful feature work, non-trivial refactoring, important bug fix, \
significant test additions. Moderate complexity and impact.
  significant — Major architectural change, large feature implementation, critical \
production fix, security patch, or work that changes system behaviour in fundamental ways. \
High complexity and/or high impact.

Consider these signals:
- Lines changed (provided in context) — more lines doesn't always mean more significant
- Title and description keywords — \"fix typo\" vs \"redesign auth flow\"
- Whether the change is additive (new feature) vs corrective (bug fix) vs structural (refactor)

Most PRs are routine. Be conservative with \"significant\" — reserve it for work that a \
team lead would want to know about specifically.
Set confidence lower (0.3-0.6) when the title/description is vague.";

/// Preamble for the topic classifier.
pub const TOPIC_PREAMBLE: &str = "\
You classify Discourse forum topics into categories for an engineering insights platform. \
This helps teams understand what their community is discussing.

Given a topic title and opening post content, assign:
  primary_category — The best-fit category from this list:
    - \"question\" — Asking for help or information
    - \"announcement\" — News, releases, updates
    - \"discussion\" — Open-ended conversation or debate
    - \"bug_report\" — Reporting a problem or defect
    - \"feature_request\" — Proposing new functionality
    - \"tutorial\" — How-to guide or walkthrough
    - \"showcase\" — Showing off work or results
    - \"blog\" — Blog post published on the forum (often tagged \"blog\")
    - \"meta\" — About the forum itself, moderation, policies
    - \"other\" — Doesn't fit the above

  secondary_category — Optional. Use if the topic clearly spans two categories \
(e.g. a bug report that is also a feature request). Omit if the primary category is sufficient.

Be specific with the rationale — mention what in the title or content led to the classification.
Set confidence lower (0.3-0.6) when the post is very short or the intent is unclear.";
