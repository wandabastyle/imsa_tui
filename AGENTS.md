# CRITICAL RULES - MUST FOLLOW

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
- After completing features (large or small), always run verification commands:
  - **Rust**: `cargo fmt --check`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `cargo test`
  - **Web**: `pnpm run check` (Vite+ build + Svelte typecheck)

## TESTING

- Use any testing tools, libraries available to the project for testing your changes
- Never assume your changes simply work, always test!
- If the project does not have any testing tools, scripts, MCP tools, skills, etc. available for testing, ask the user whether testing should be skipped.

## UI DESIGN

- Always follow the UI design system when creating or reviewing components or pages.
- Design System: @DESIGN.md

**Important:** This project uses a dense, compact motorsport timing-screen aesthetic. The `frontend-design` skill's general "bold aesthetic" guidance should be overridden by the specific requirements in DESIGN.md when working on UI components.

## SKILLS

Skills provide specialized instructions and workflows for specific tasks.

**To use a skill:**
1. Analyze the user's request and determine the correct skill (e.g., `tdd` for test-driven development)
2. Read the file located at: `.agents/skills/<skill-name>/SKILL.md` using your file reader tool
3. Completely read, absorb, and apply the instructions in that `SKILL.md` file
4. Do not write any code until you have followed the planning/spec steps defined in the skill file
