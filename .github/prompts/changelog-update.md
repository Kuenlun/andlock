**Role:** Expert Rust Software Engineer and technical writer.

**Task:** Enrich the terse entries that release-plz wrote into `CHANGELOG.md` for the release it is preparing, replacing them with detailed, user-facing descriptions drawn from the actual commit history. Leave every other part of the file untouched.

**Target section:**

- On a release-plz branch, the target is the newly inserted `## [x.y.z](https://github.com/Kuenlun/andlock/compare/vA...vB) - YYYY-MM-DD` block at the top of the version list, not `## [Unreleased]`.
- Do not rename the header, do not change the date, do not rewrite the compare URL, do not move the block. Rewrite only the bullets beneath it.
- If explicitly asked to enrich `## [Unreleased]` instead, target that one and apply the same rules.

**Investigation (do this before writing):**

1. **Read the current `CHANGELOG.md`** end to end to learn the established style, punctuation, section order, and level of detail. Match it.
2. **Find the previous tag** with `git describe --tags --abbrev=0` (or `git tag --sort=-v:refname | head -n 1` if the former fails). On a release-plz branch, `HEAD` is the release commit itself, so it is included in the range.
3. **Collect the commits** with:
   ```
   git log <previous-tag>..HEAD --format="%H%n%s%n%b%n---END---"
   ```
   The `---END---` delimiter lets you parse bodies that contain blank lines. Each squash-merge subject ends in ` (#N)`, the PR number. Use that `N` to build the link `https://github.com/Kuenlun/andlock/pull/N`. Never invent a PR number, never fetch GitHub pages.
4. **When a commit body is itself terse** (the author wrote no rich description), fall back to inspecting the net diff of that commit with `git show <sha>` or `git log -p -1 <sha>`. Never guess from the subject alone.

**Classification (Keep a Changelog, https://keepachangelog.com/en/1.0.0/):**

- `feat`, new public API, new CLI flag, new subcommand → **Added**
- behavioural change visible to users, algorithm swap that changes output or performance characteristics, user-visible rename → **Changed**
- `fix` → **Fixed**
- removed feature, removed flag, removed public API → **Removed**
- deprecation of a still-working feature → **Deprecated**
- security patch → **Security**
- pure internal refactor with no observable user effect, CI, release machinery, tooling, or test-only changes → **skip entirely**, do not map to any section

**Writing rules for new entries:**

- One entry per logical change. A single PR may produce multiple entries if it touches multiple sections.
- Include concrete, actionable detail: flag names, subcommand names, public identifiers, file or module names when user-facing, numeric bounds, complexity bounds, guard conditions, default values.
- Imperative sentence case for the leading verb: `Add`, `Extract`, `Support`, `Use`, `Replace`, `Raise`, `Widen`. Not past tense (`Added`, `Supported`).
    - Good: `Add closed-form path-counting formula for unconstrained grids, reducing pattern counting from O(n·2ⁿ) to O(n).`
    - Bad: `Supported zero-sized grid dimensions.` (past tense, no detail)
- End every entry with the PR link in the exact form `([#N](https://github.com/Kuenlun/andlock/pull/N))`.
- Match the level of detail of surrounding entries in the same section across prior releases. Do not pad, do not hand-wave.
- Do not use em dashes (`—`) or en dashes (`–`). Do not use semicolons (`;`). Prefer short sentences and commas. Historical entries that already use these characters must be left as-is, the rule only applies to entries you write now.

**Output constraints:**

- Apply the edits with the `Edit` tool directly against `CHANGELOG.md`. Do not print the whole file back and do not wrap output in code fences.
- Within the target section, emit sections in this order, omitting any that would be empty: **Added → Changed → Deprecated → Removed → Fixed → Security**. Never emit `Other`, it is not a valid Keep a Changelog section.
- Historical `Other` sections from older releases (for example in `[0.2.0]`) must be left completely untouched, including their wording and punctuation. The "never emit Other" rule applies only to new entries you write.
- Preserve blank lines and header formatting exactly as release-plz produced them outside your edits.
- Do not invent information that is not present in the commit history or the diff of the relevant commit.
