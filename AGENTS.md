# CRITICAL RULES - MUST FOLLOW

## GENERAL PRINCIPLES

- **No `#[allow(...)]` attributes**: Never suppress Clippy or compiler warnings with `#[allow(...)]` attributes. This includes (but is not limited to):
  - `#[allow(dead_code)]` - Dead code should be removed rather than suppressed
  - `#[allow(clippy::too_many_arguments)]` - Refactor to use context structs instead
  - Any other `#[allow(clippy::...)]` - Fix the underlying issue

  If there's a valid reason to keep code that triggers warnings (e.g., for future use or API completeness), add a comment explaining why and fix the root cause (removing dead code, using constants/structs, or refactoring) rather than suppressing warnings.

## Rust `#[must_use]` policy

Use `#[must_use]` selectively when ignoring a return value would likely be a bug.

Good candidates:
- pure getters that return computed or stored values
- boolean check helpers, such as `is_*`, `has_*`, `can_*`
- functions returning `Option<T>`, `Result<T, E>`, or other meaningful computed values
- conversion/formatting helpers that return a new value
- builder-style methods returning `Self`
- methods returning a modified/new value instead of mutating in place

Avoid adding `#[must_use]` blindly to every function with a return value.

Do not add it when:
- the function exists mainly for side effects
- ignoring the return value is a normal/valid use case
- the annotation only adds noise to internal glue code

When Clippy suggests `clippy::must_use_candidate`, prefer adding `#[must_use]` if it matches the rules above. Otherwise, allow the lint locally with a brief reason.

Example:

```rust
#[must_use]
pub fn is_finished(&self) -> bool {
    self.finished
}

#[must_use]
pub fn with_quality(mut self, quality: Quality) -> Self {
    self.quality = quality;
    self
}
```

For builder-style methods returning `Self`, treat missing `#[must_use]` as a real issue unless there is a clear reason not to.

## RESPONSES

- Keep responses concise and to the point - unless the user asks otherwise

## PLANNING MODE

- Always ask clarifying questions
- Never assume design, tech stack or features
- Use deep-dive sub-agents to assist with research
- Use deep-dive sub-agents to review the different aspects of your plan before presenting to the user

## CHANGE / EDIT MODE

- Never implement features yourself when possible - use sub-agents!
- Identify changes from the plan that can be implemented in parallel, and use sub-agents to implement the features efficiently
- When using sub-agents to implement features, act as a coordinator only
- Use the best model for the task - premium models for complex tasks (like coding) and mid-tier models for simpler tasks, like documentation
- **Lint rules first**: Agents and subagents must read lint/typecheck output completely before making any code changes - don't assume you know the rules, read what the linter actually says
- After completing features (large or small), always run verification commands:
  - **Rust**: `cargo fmt --check`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `cargo test`
  - **Web**: `pnpm run check` (Vite+ build + React/TypeScript check)

## TESTING

- Use any testing tools, libraries available to the project for testing your changes
- Never assume your changes simply work, always test!
- If the project does not have any testing tools, scripts, MCP tools, skills, etc. available for testing, ask the user whether testing should be skipped.

## VERSION BUMPING

- When making bug fixes or small improvements that could reasonably constitute a patch release, bump the project version in **both** `Cargo.toml` and `web/package.json` together. If dependency versions changed, run `cargo update -p <changed-crate>` to keep `Cargo.lock` in sync.
- Only bump the minor/major version when the changeset is large enough or includes breaking changes. When in doubt, patch bump.
- Keep `Cargo.toml` and `web/package.json` in sync.

## UI DESIGN

- Always follow the UI design system when creating or reviewing components or pages.
- Design System: @DESIGN.md

**Important:** This project uses a dense, compact motorsport timing-screen aesthetic. The `frontend-design` skill's general "bold aesthetic" guidance should be overridden by the specific requirements in DESIGN.md when working on UI components.

## LINT RULES

Agents and subagents must read lint/typecheck output completely before making any code changes - don't assume you know the rules, read what the linter actually says.

**Web lint rules:** `web/.oxlintrc.json`
- All categories enabled: `correctness`, `suspicious`, `pedantic`, `perf`, `style`, `restriction`, `nursery`
- No ESLint/oxc configs; rules explicitly defined in oxlintrc
- No eslint config file (like `.eslintrc.js` or `eslint.config.js`)

**Rust lint rules:** From `[lints.clippy]` section in `Cargo.toml`
- `all`, `cargo`, `nursery`, `pedantic` = `warn`
- `multiple_crate_versions` = `allow` (transitive deps ecosystem issue)
- Run with: `cargo clippy --all-targets --no-default-features -- -D warnings`

## SKILLS

Skills provide specialized instructions and workflows for specific tasks.

**To use a skill:**
1. Analyze the user's request and determine the correct skill (e.g., `tdd` for test-driven development)
2. Read the file located at: `.agents/skills/<skill-name>/SKILL.md` using your file reader tool
3. Completely read, absorb, and apply the instructions in that `SKILL.md` file
4. Do not write any code until you have followed the planning/spec steps defined in the skill file
