**Role:** Expert Rust Software Engineer and technical writer.

**Task:** Enrich the terse entries that release-plz wrote into `CHANGELOG.md` for the release it is preparing, replacing them with detailed, user-facing descriptions drawn from the actual commit history. Leave every other part of the file untouched.

**Target section:**

- On a release-plz branch, the target is the newly inserted `## [x.y.z](https://github.com/Kuenlun/andlock/compare/vA...vB) - YYYY-MM-DD` block at the top of the version list, not `## [Unreleased]`.
- Do not rename the `##` header, do not change the date, do not rewrite the compare URL, do not move the block. Within the block you may freely add, remove, rename, and reorder the `###` subsection headers and their bullets.
- If a release was retracted, mark its `##` header with a trailing `[YANKED]` tag (uppercase) per Keep a Changelog, for example `## [0.3.1] - 2026-05-04 [YANKED]`. Do not invent yanks, only apply this when explicitly told the release was pulled.
- If explicitly asked to enrich `## [Unreleased]` instead, target that one and apply the same rules.

**Investigation (do this before writing):**

1. **Read the current `CHANGELOG.md`** end to end to learn the established style, punctuation, section order, and level of detail. Match it.
2. **Find the previous tag** with `git describe --tags --abbrev=0` (or `git tag --sort=-v:refname | head -n 1` if the former fails). On a release-plz branch, `HEAD` is the release commit itself, so it is included in the range.
3. **Collect the commits** with:
   ```
   git log <previous-tag>..HEAD --format="%H%n%s%n%b%n---END---"
   ```
   The `---END---` delimiter lets you parse bodies that contain blank lines. The `master` branch is protected and every change lands via a squash-merged PR, so every subject in this range (except the release-plz release commit itself) ends in ` (#N)`, the PR number. Use that `N` to build the link `https://github.com/Kuenlun/andlock/pull/N`. Never invent a PR number, never fetch GitHub pages. If a subject in the range has no `(#N)`, treat it as the release commit and skip it.
4. **When a commit body is itself terse** (the author wrote no rich description), fall back to inspecting the net diff of that commit with `git show <sha>` or `git log -p -1 <sha>`. Never guess from the subject alone.
5. **If there is no previous tag** (first release of the project), use the full history range `HEAD` instead of `<previous-tag>..HEAD`.

**Classification (Keep a Changelog 1.0.0, https://keepachangelog.com/en/1.0.0/):**

The six canonical section names and their meanings, verbatim from the spec:

- **Added** for new features.
- **Changed** for changes in existing functionality.
- **Deprecated** for soon-to-be-removed features.
- **Removed** for now-removed features.
- **Fixed** for any bug fixes.
- **Security** in case of vulnerabilities.

Mapping rules for this repo:

- `feat`, new public API, new CLI flag, new subcommand → **Added**
- behavioural change visible to users, algorithm swap that changes output or performance characteristics, user-visible rename → **Changed**
- `fix` → **Fixed**
- removed feature, removed flag, removed public API → **Removed**
- deprecation of a still-working feature → **Deprecated**
- security patch → **Security**
- pure internal refactor with no observable user effect, CI, release machinery, tooling, or test-only changes → **skip entirely**, do not map to any section

**Breaking changes (SemVer 2.0.0, https://semver.org/spec/v2.0.0.html):**

- A commit is breaking when its Conventional Commits type carries `!` (e.g. `feat!:`, `refactor!:`) or its body contains a `BREAKING CHANGE:` footer.
- Place the entry in the section that describes the _kind_ of change (usually **Changed** or **Removed**, sometimes **Added** for an incompatible new requirement) and prefix the bullet with `**BREAKING:**`. Do not invent a separate "Breaking" section, it is not part of Keep a Changelog.
- Do not assert what version bump SemVer requires, release-plz already decides that. While the project is on `0.y.z` (initial development per SemVer clause 4), breaking changes do not necessarily force a major bump.

**Writing rules for new entries:**

Note: Keep a Changelog 1.0.0 itself does not mandate tense or sentence form. The rules below are repo conventions applied for internal consistency.

- One entry per logical change. A single PR may produce multiple entries if it touches multiple sections.
- Include concrete, actionable detail: flag names, subcommand names, public identifiers, file or module names when user-facing, numeric bounds, complexity bounds, guard conditions, default values.
- Imperative sentence case for the leading verb: `Add`, `Extract`, `Support`, `Use`, `Replace`, `Raise`, `Widen`. Not past tense (`Added`, `Supported`).
  - Good: `Add closed-form path-counting formula for unconstrained grids, reducing pattern counting from O(n·2ⁿ) to O(n).`
  - Bad: `Supported zero-sized grid dimensions.` (past tense, no detail)
- Identifiers (type names, file names, crate names, flag names) keep their original capitalization even when the leading verb is sentence case.
- End every entry with the PR link in the exact form `([#N](https://github.com/Kuenlun/andlock/pull/N))`.
- Match the level of detail of surrounding entries in the same section across prior releases. Do not pad, do not hand-wave.
- Do not use em dashes (`—`) or en dashes (`–`). Do not use semicolons (`;`). Prefer short sentences and commas. Historical entries that already use these characters must be left as-is, the rule only applies to entries you write now.

**Output constraints:**

- Apply the edits with the `Edit` tool directly against `CHANGELOG.md`. Do not print the whole file back and do not wrap output in code fences.
- Keep a Changelog 1.0.0 does not mandate an order between the six subsections, but for internal consistency emit them in this order, omitting any that would be empty: **Added → Changed → Deprecated → Removed → Fixed → Security**. Never emit `Other`, it is not one of the canonical Keep a Changelog sections.
- Historical `Other` sections from older releases (for example in `[0.2.0]`) must be left completely untouched, including their wording and punctuation. The "never emit Other" rule applies only to new entries you write.
- Preserve blank lines and header formatting exactly as release-plz produced them outside your edits.
- Do not invent information that is not present in the commit history or the diff of the relevant commit.
